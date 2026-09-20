//! Cranelift lowering and native object emission.
//!
//! Cranelift rather than LLVM for M0 (#3): trivial to embed, no C++ build
//! dependency, and compile speed suits the feedback loop. LLVM arrives later
//! for release-quality codegen — the plan is to ship both, as rustc does.
//!
//! Everything this module can reject, `lex-sys-ir` has already rejected, so the
//! only errors here are environment errors: an unsupported host, or a Cranelift
//! verifier complaint, which is a compiler bug rather than a program error.

use std::fmt;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, InstBuilder, MemFlags, StackSlotData, StackSlotKind, Value, types,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, isa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use lex_sys_ir::{
    Arm, BinOp, Builtin, Callee, Expr, Func, FuncId as IrFuncId, Place, Program, Slot, Stmt,
    TypeInfo, terminates,
};
use lex_sys_types::{DefId, Type};
use target_lexicon::Triple;

/// The machine types a lex-sys type is held in, in field order.
///
/// A struct is **scalarised**: it is not a block of memory with a layout, it
/// is its leaf fields, each in its own register or stack argument. M1 has no
/// references and no recursive structs, so every value is a finite tree of
/// scalars and this always terminates.
///
/// That also means M1 makes no layout decisions at all. Deterministic layout
/// is a commitment (#1) and `docs/defined-behaviour.md` is where it gets
/// decided; choosing one here to make the backend easier would be choosing it
/// by accident.
///
/// `bool` is an `i8` because that is what Cranelift's `icmp` produces, so a
/// comparison *is* a `bool` with nothing to convert. M0 widened every
/// comparison to `i64` and called it an `int`; the type system now says what
/// was always true about the value.
fn leaves_into(ty: &Type, program: &Program, pointer: types::Type, out: &mut Vec<types::Type>) {
    match ty {
        Type::Int => out.push(types::I64),
        Type::Bool => out.push(types::I8),
        // A generic type's members are written in terms of its parameters, so
        // they are substituted here rather than monomorphised: `Pair[int,
        // bool]` and `Pair[bool, int]` are two leaf layouts of one
        // declaration. Only *functions* are copied per instantiation.
        Type::Named(def, args) => match program.type_info(*def) {
            TypeInfo::Struct { fields, .. } => {
                for (_, field) in fields {
                    leaves_into(&field.substitute(args, &[]), program, pointer, out);
                }
            }
            // An enum is a tag followed by *every* variant's payload, each in
            // its own leaves. That is wasteful and deliberately so: overlaying
            // the payloads is a layout decision, and M1 owns no layout
            // decisions (#1, `docs/defined-behaviour.md` in M3). The tag is an
            // `i64` for the same reason — picking the narrowest integer that
            // fits would be choosing a representation.
            TypeInfo::Enum { variants, .. } => {
                out.push(types::I64);
                for (_, payload) in variants {
                    for ty in payload {
                        leaves_into(&ty.substitute(args, &[]), program, pointer, out);
                    }
                }
            }
        },
        // A reference is one pointer, whatever it points at. Regions are
        // erased: which `borrow` block a reference came from is a fact the
        // checker used and the machine has no use for.
        Type::Ref { .. } => out.push(pointer),
        other => {
            unreachable!("`{other:?}` reached the backend; the checker should have refused it")
        }
    }
}

fn leaves(ty: &Type, program: &Program, pointer: types::Type) -> Vec<types::Type> {
    let mut out = Vec::new();
    leaves_into(ty, program, pointer, &mut out);
    out
}

fn leaf_count(ty: &Type, program: &Program, pointer: types::Type) -> u32 {
    leaves(ty, program, pointer).len() as u32
}

/// How many leaves a return value may have before it travels through memory.
///
/// Two is what x86-64's SystemV ABI gives back in registers, and Cranelift
/// refuses outright above it. Rather than let the limit differ per target —
/// aarch64 would allow eight — the same rule applies everywhere, so a program
/// that compiles on one target compiles on the other.
const MAX_RETURN_LEAVES: usize = 2;

/// Does a value of this type come back through memory rather than in
/// registers?
fn returns_indirectly(ty: &Type, program: &Program, pointer: types::Type) -> bool {
    leaf_count(ty, program, pointer) as usize > MAX_RETURN_LEAVES
}

/// Byte offset of a leaf in an indirect return buffer.
///
/// One slot of pointer width per leaf: this is a private arrangement between a
/// lex-sys function and its lex-sys caller, not a layout the language
/// promises. `docs/defined-behaviour.md` still owns that question in M3, and
/// nothing here is observable to a program.
const RETURN_SLOT_STRIDE: i32 = 8;

/// Every lex-sys function is emitted under this prefix, so a program may define
/// a function called `write` or `exit` without colliding with libc.
const PREFIX: &str = "lexs_";

#[derive(Debug)]
pub struct CodegenError(String);

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CodegenError {}

impl From<String> for CodegenError {
    fn from(s: String) -> Self {
        CodegenError(s)
    }
}

/// The host triple this compiler emits for. M0 is host-only; cross-compilation
/// is not a milestone yet.
pub fn host_triple() -> Triple {
    Triple::host()
}

/// Compile a program to the bytes of a native object file for the host.
///
/// `entry` names the lex-sys function that becomes the process entry point: a C
/// `main` is synthesised around it, truncating its `int` result to the `int`
/// the platform's exit status is.
pub fn compile_object(program: &Program, entry: &str) -> Result<Vec<u8>, CodegenError> {
    compile_object_for(program, entry, host_triple())
}

/// Compile a program to the bytes of an object file for an explicit target.
///
/// M0 builds only for the host — there is no cross-compilation story and no
/// driver flag for one. This exists so the object *format's* conventions can be
/// tested from any host: symbol mangling differs between ELF and Mach-O, and
/// getting it wrong is a link failure nobody on Linux will ever see.
pub fn compile_object_for(
    program: &Program,
    entry: &str,
    triple: Triple,
) -> Result<Vec<u8>, CodegenError> {
    let mut flags = settings::builder();
    // Position-independent code: both default targets link PIE by default.
    flags.set("is_pic", "true").map_err(|e| CodegenError(e.to_string()))?;
    // Deterministic output matters more here than the last few percent (#1).
    flags.set("opt_level", "speed").map_err(|e| CodegenError(e.to_string()))?;
    let flags = settings::Flags::new(flags);

    let isa = isa::lookup(triple.clone())
        .map_err(|e| CodegenError(format!("unsupported host `{triple}`: {e}")))?
        .finish(flags)
        .map_err(|e| CodegenError(e.to_string()))?;

    let builder = ObjectBuilder::new(isa, "lex-sys", default_libcall_names())
        .map_err(|e| CodegenError(e.to_string()))?;
    let mut module = ObjectModule::new(builder);

    let mut emitter = Emitter { module, program };
    emitter.emit(entry)?;
    module = emitter.module;

    module.finish().emit().map_err(|e| CodegenError(e.to_string()))
}

struct Emitter<'a> {
    module: ObjectModule,
    program: &'a Program,
}

impl<'a> Emitter<'a> {
    /// Declare and define every function, then the C entry point.
    ///
    /// Symbols are declared under their plain C names throughout. The
    /// platform's own convention is applied underneath us: `object` picks a
    /// `Mangling` from the binary format when the object is created, and
    /// Mach-O's adds the leading underscore to every text and data symbol. So
    /// `main` becomes `_main` on darwin and stays `main` on ELF, and adding one
    /// here as well produced `__main` and a link the darwin linker could not
    /// resolve.
    fn emit(&mut self, entry: &str) -> Result<(), CodegenError> {
        let call_conv = self.module.isa().default_call_conv();
        let pointer = self.module.isa().pointer_type();
        // Bound once: the body emitter borrows the module mutably, so the
        // program has to be reached through a separate binding.
        let program = self.program;

        // Declare every lex-sys function first: calls are resolved against
        // declarations, so definition order in the file never matters.
        let mut declared: Vec<FuncId> = Vec::with_capacity(self.program.funcs.len());
        for func in &self.program.funcs {
            let mut sig = self.module.make_signature();
            sig.call_conv = call_conv;
            for slot in &func.slots[..func.n_params as usize] {
                for leaf in leaves(slot, self.program, pointer) {
                    sig.params.push(AbiParam::new(leaf));
                }
            }
            if returns_indirectly(&func.ret, self.program, pointer) {
                // The caller allocates the buffer and passes its address
                // first; nothing comes back in registers.
                sig.params.insert(0, AbiParam::new(pointer));
            } else {
                for leaf in leaves(&func.ret, self.program, pointer) {
                    sig.returns.push(AbiParam::new(leaf));
                }
            }
            let id = self
                .module
                .declare_function(&format!("{PREFIX}{}", func.name), Linkage::Local, &sig)
                .map_err(|e| CodegenError(e.to_string()))?;
            declared.push(id);
        }

        // libc's `putchar(int) -> int`: `int` is 32-bit on both default
        // targets, so the argument narrows and the result widens at the edge.
        let mut putchar_sig = self.module.make_signature();
        putchar_sig.call_conv = call_conv;
        putchar_sig.params.push(AbiParam::new(types::I32));
        putchar_sig.returns.push(AbiParam::new(types::I32));
        let putchar = self
            .module
            .declare_function(Builtin::PutChar.symbol(), Linkage::Import, &putchar_sig)
            .map_err(|e| CodegenError(e.to_string()))?;

        let mut ctx = Context::new();
        let mut fb_ctx = FunctionBuilderContext::new();

        for (index, func) in self.program.funcs.iter().enumerate() {
            ctx.clear();
            ctx.func.signature =
                self.module.declarations().get_function_decl(declared[index]).signature.clone();

            {
                let builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
                let mut body =
                    BodyEmitter::new(builder, &mut self.module, &declared, putchar, func, program);
                body.emit_func(func);
                body.builder.finalize();
            }

            self.module
                .define_function(declared[index], &mut ctx)
                .map_err(|e| CodegenError(format!("in `{}`: {e}", func.name)))?;
        }

        let entry_id = self
            .program
            .find(entry)
            .ok_or_else(|| CodegenError(format!("no function named `{entry}` to use as entry")))?;
        self.emit_c_main(entry_id, &declared, &mut ctx, &mut fb_ctx)
    }

    /// Synthesise `int main(void)`, which calls the lex-sys entry function and
    /// truncates its result to the platform's exit status.
    fn emit_c_main(
        &mut self,
        entry: IrFuncId,
        declared: &[FuncId],
        ctx: &mut Context,
        fb_ctx: &mut FunctionBuilderContext,
    ) -> Result<(), CodegenError> {
        let mut sig = self.module.make_signature();
        sig.call_conv = self.module.isa().default_call_conv();
        sig.returns.push(AbiParam::new(types::I32));
        let main = self
            .module
            .declare_function("main", Linkage::Export, &sig)
            .map_err(|e| CodegenError(e.to_string()))?;

        ctx.clear();
        ctx.func.signature = sig;
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, fb_ctx);
            let block = builder.create_block();
            builder.switch_to_block(block);
            builder.seal_block(block);

            let callee = self.module.declare_func_in_func(declared[entry.0 as usize], builder.func);
            let call = builder.ins().call(callee, &[]);
            let status = builder.inst_results(call)[0];
            let status = builder.ins().ireduce(types::I32, status);
            builder.ins().return_(&[status]);
            builder.finalize();
        }
        self.module.define_function(main, ctx).map_err(|e| CodegenError(e.to_string()))?;
        Ok(())
    }
}

struct BodyEmitter<'a, 'f> {
    builder: FunctionBuilder<'f>,
    module: &'a mut ObjectModule,
    declared: &'a [FuncId],
    putchar: FuncId,
    func: &'a Func,
    program: &'a Program,
    /// The buffer this function writes its result into, when its return type
    /// is too wide for registers.
    return_pointer: Option<Value>,
    pointer: types::Type,
    /// Where each slot's leaves begin among the function's variables.
    slot_base: Vec<u32>,
    /// Slot leaves occupy the variables below this; temporaries the backend
    /// needs for its own purposes are numbered from here.
    next_var: u32,
}

impl<'a, 'f> BodyEmitter<'a, 'f> {
    fn new(
        builder: FunctionBuilder<'f>,
        module: &'a mut ObjectModule,
        declared: &'a [FuncId],
        putchar: FuncId,
        func: &'a Func,
        program: &'a Program,
    ) -> Self {
        let pointer = module.isa().pointer_type();
        let mut slot_base = Vec::with_capacity(func.slots.len());
        let mut next_var = 0;
        for slot in &func.slots {
            slot_base.push(next_var);
            next_var += leaf_count(slot, program, pointer);
        }
        Self {
            builder,
            module,
            declared,
            putchar,
            func,
            program,
            return_pointer: None,
            pointer,
            slot_base,
            next_var,
        }
    }

    /// Write leaf values into an indirect return buffer.
    fn store_leaves(&mut self, address: Value, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let offset = index as i32 * RETURN_SLOT_STRIDE;
            self.builder.ins().store(MemFlags::trusted(), *value, address, offset);
        }
    }

    /// Read leaf values back out of one.
    fn load_leaves(&mut self, address: Value, kinds: &[types::Type]) -> Vec<Value> {
        kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let offset = index as i32 * RETURN_SLOT_STRIDE;
                self.builder.ins().load(*kind, MemFlags::trusted(), address, offset)
            })
            .collect()
    }

    /// Reserve a buffer big enough for a value of this type.
    fn return_buffer(&mut self, ty: &Type) -> Value {
        let size = leaf_count(ty, self.program, self.pointer) * RETURN_SLOT_STRIDE as u32;
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            3,
        ));
        let pointer = self.pointer;
        self.builder.ins().stack_addr(pointer, slot, 0)
    }

    /// A fresh variable the backend owns, numbered above every slot leaf.
    fn temporary(&mut self, ty: types::Type) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        self.builder.declare_var(var, ty);
        var
    }

    fn emit_func(&mut self, func: &Func) {
        let entry = self.builder.create_block();
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);
        self.builder.seal_block(entry);

        // Every leaf of every slot is a Cranelift variable; the SSA builder
        // turns them back into values. Parameters take their incoming values,
        // everything else starts at zero so no path can observe an undefined
        // slot.
        for (index, ty) in func.slots.iter().enumerate() {
            let base = self.slot_base[index];
            for (offset, leaf) in leaves(ty, self.program, self.pointer).into_iter().enumerate() {
                self.builder.declare_var(Variable::from_u32(base + offset as u32), leaf);
            }
        }

        // When the result travels through memory the address arrives first,
        // so every parameter leaf sits one position later.
        let indirect = returns_indirectly(&func.ret, self.program, self.pointer);
        if indirect {
            self.return_pointer = Some(self.builder.block_params(entry)[0]);
        }
        let shift = usize::from(indirect);

        let param_leaves: u32 = func.slots[..func.n_params as usize]
            .iter()
            .map(|ty| leaf_count(ty, self.program, self.pointer))
            .sum();
        for index in 0..param_leaves {
            let value = self.builder.block_params(entry)[index as usize + shift];
            self.builder.def_var(Variable::from_u32(index), value);
        }
        for (index, ty) in func.slots.iter().enumerate().skip(func.n_params as usize) {
            let base = self.slot_base[index];
            for (offset, leaf) in leaves(ty, self.program, self.pointer).into_iter().enumerate() {
                let zero = self.builder.ins().iconst(leaf, 0);
                self.builder.def_var(Variable::from_u32(base + offset as u32), zero);
            }
        }

        let terminated = self.stmts(&func.body);
        if !terminated {
            // Unreachable in a well-formed program: lowering proved every path
            // returns. Emitted so the block is filled whatever happens.
            self.return_zero();
        }
    }

    /// Return a zero of the function's return type, however many leaves it has.
    fn return_zero(&mut self) {
        let zeros: Vec<Value> = leaves(&self.func.ret, self.program, self.pointer)
            .into_iter()
            .map(|leaf| self.builder.ins().iconst(leaf, 0))
            .collect();
        self.emit_return(zeros);
    }

    /// Hand back a result, in registers or through the caller's buffer.
    fn emit_return(&mut self, values: Vec<Value>) {
        match self.return_pointer {
            Some(address) => {
                self.store_leaves(address, &values);
                self.builder.ins().return_(&[]);
            }
            None => {
                self.builder.ins().return_(&values);
            }
        }
    }

    /// Emit a statement list; returns whether control left via `return`.
    fn stmts(&mut self, stmts: &[Stmt]) -> bool {
        for stmt in stmts {
            match stmt {
                Stmt::Store { place, value } => {
                    let values = self.expr(value);
                    self.write(place, values);
                }
                Stmt::Eval(expr) => {
                    self.expr(expr);
                }
                Stmt::Return(expr) => {
                    let values = self.expr(expr);
                    self.emit_return(values);
                    return true;
                }
                Stmt::If { cond, then_body, else_body } => {
                    if self.if_stmt(cond, then_body, else_body) {
                        return true;
                    }
                }
                Stmt::While { cond, body } => self.while_stmt(cond, body),
                Stmt::Borrow { referent, reference, unique, body } => {
                    if self.borrow_stmt(*referent, *reference, *unique, body) {
                        return true;
                    }
                }
                Stmt::Match { scrutinee, def, args, arms } => {
                    if self.match_stmt(scrutinee, *def, args, arms) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `borrow x as &r in { .. }` — give `x` a home in memory and point at it.
    ///
    /// A reference has to be an address, and until now nothing did: a slot
    /// lives in SSA variables, which have none. So the referent's leaves are
    /// spilled into a buffer for the duration of the block and the reference
    /// holds that buffer's address.
    ///
    /// Nothing is written back when the block closes, and nothing needs to
    /// be: the checker froze the referent for the whole region, so the
    /// variables and the buffer cannot have drifted apart. A unique borrow
    /// will need the copy back, and the buffer is where it will come from.
    fn borrow_stmt(
        &mut self,
        referent: Slot,
        reference: Slot,
        unique: bool,
        body: &[Stmt],
    ) -> bool {
        let ty = self.func.slots[referent.0 as usize].clone();
        let buffer = self.return_buffer(&ty);
        let base = self.slot_base[referent.0 as usize];
        let count = leaf_count(&ty, self.program, self.pointer);
        let values: Vec<Value> = (0..count)
            .map(|offset| self.builder.use_var(Variable::from_u32(base + offset)))
            .collect();
        self.store_leaves(buffer, &values);
        self.builder.def_var(Variable::from_u32(self.slot_base[reference.0 as usize]), buffer);
        let returned = self.stmts(body);

        // A unique borrow may have written through the reference, and the
        // variables still hold what the buffer held on the way in. Reading
        // them back is sound because the checker *locked* the referent for
        // the whole block: nothing else could touch it, so the buffer is the
        // only version that moved.
        //
        // Skipped when the body returned, because nothing after this runs --
        // and Cranelift has no block to put the loads in.
        if unique && !returned {
            let kinds = leaves(&ty, self.program, self.pointer);
            let restored = self.load_leaves(buffer, &kinds);
            for (offset, value) in restored.into_iter().enumerate() {
                self.builder.def_var(Variable::from_u32(base + offset as u32), value);
            }
        }
        returned
    }

    /// Write leaf values into a place.
    ///
    /// A whole local is `def_var` per leaf, as it always was. A field through
    /// a reference is the same arithmetic as reading one, running the other
    /// way: find where the field starts among the referent's leaves, and
    /// store there.
    fn write(&mut self, place: &Place, values: Vec<Value>) {
        match place {
            Place::Slot(slot) => {
                let base = self.slot_base[slot.0 as usize];
                for (offset, value) in values.into_iter().enumerate() {
                    self.builder.def_var(Variable::from_u32(base + offset as u32), value);
                }
            }
            Place::Field { base, def, args, index } => {
                let address = self.scalar(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field write to an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let offset = start as i32 * RETURN_SLOT_STRIDE;
                for (i, value) in values.into_iter().enumerate() {
                    let at = offset + i as i32 * RETURN_SLOT_STRIDE;
                    self.builder.ins().store(MemFlags::trusted(), value, address, at);
                }
            }
        }
    }

    fn if_stmt(&mut self, cond: &Expr, then_body: &[Stmt], else_body: &[Stmt]) -> bool {
        let cond = self.scalar(cond);
        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        let merge = self.builder.create_block();

        // The condition is an `i8` holding 0 or 1, which is what `brif` tests.
        // M0 tested any integer for non-zero; the checker now guarantees a
        // `bool` got here.
        self.builder.ins().brif(cond, then_block, &[], else_block, &[]);

        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        if !self.stmts(then_body) {
            self.builder.ins().jump(merge, &[]);
        }

        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        if !self.stmts(else_body) {
            self.builder.ins().jump(merge, &[]);
        }

        let both_returned = terminates(then_body) && !else_body.is_empty() && terminates(else_body);
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        if both_returned {
            // Nothing branches here. Fill it anyway: an empty block is not a
            // legal function, and this costs instructions the linker drops.
            self.return_zero();
        }
        both_returned
    }

    fn while_stmt(&mut self, cond: &Expr, body: &[Stmt]) {
        let header = self.builder.create_block();
        let body_block = self.builder.create_block();
        let exit = self.builder.create_block();

        self.builder.ins().jump(header, &[]);
        // The header stays unsealed until the back edge is emitted.
        self.builder.switch_to_block(header);
        let cond = self.scalar(cond);
        self.builder.ins().brif(cond, body_block, &[], exit, &[]);

        self.builder.switch_to_block(body_block);
        self.builder.seal_block(body_block);
        if !self.stmts(body) {
            self.builder.ins().jump(header, &[]);
        }
        self.builder.seal_block(header);

        self.builder.switch_to_block(exit);
        self.builder.seal_block(exit);
    }

    /// Where a variant's payload starts among an enum's leaves, and how many
    /// leaves each payload position occupies.
    fn variant_layout(&self, def: DefId, args: &[Type], variant: u32) -> (u32, Vec<u32>) {
        let TypeInfo::Enum { variants, .. } = self.program.type_info(def) else {
            unreachable!("a variant of a struct should have been refused");
        };
        let width = |ty: &Type| leaf_count(&ty.substitute(args, &[]), self.program, self.pointer);
        // One for the tag, then every earlier variant's payload.
        let mut offset = 1;
        for (_, payload) in &variants[..variant as usize] {
            offset += payload.iter().map(&width).sum::<u32>();
        }
        let widths = variants[variant as usize].1.iter().map(&width).collect();
        (offset, widths)
    }

    /// Lower a `match` to a chain of tag tests.
    ///
    /// A jump table would be faster and is the obvious later move; a chain is
    /// what M1 needs and is easier to be sure of. Returns whether every arm
    /// returned, which makes the whole `match` a terminator.
    fn match_stmt(&mut self, scrutinee: &Expr, def: DefId, args: &[Type], arms: &[Arm]) -> bool {
        let values = self.expr(scrutinee);
        let tag = values[0];
        let merge = self.builder.create_block();

        let mut all_returned = true;
        // Whether the fall-through chain still has an open block. A wildcard
        // arm closes it, because nothing can follow one.
        let mut open = true;

        for arm in arms {
            if !open {
                break;
            }
            match arm.variant {
                Some(variant) => {
                    let body_block = self.builder.create_block();
                    let next = self.builder.create_block();
                    let matched =
                        self.builder.ins().icmp_imm(IntCC::Equal, tag, i64::from(variant));
                    self.builder.ins().brif(matched, body_block, &[], next, &[]);

                    self.builder.switch_to_block(body_block);
                    self.builder.seal_block(body_block);
                    self.bind_payload(def, args, variant, arm, &values);
                    let returned = self.stmts(&arm.body);
                    if !returned {
                        self.builder.ins().jump(merge, &[]);
                    }
                    all_returned &= returned;

                    self.builder.switch_to_block(next);
                    self.builder.seal_block(next);
                }
                None => {
                    // The wildcard binds nothing and needs no test: it runs
                    // right here, in the block the chain fell through to.
                    let returned = self.stmts(&arm.body);
                    if !returned {
                        self.builder.ins().jump(merge, &[]);
                    }
                    all_returned &= returned;
                    open = false;
                }
            }
        }

        if open {
            // The checker proved the arms exhaustive, so this is unreachable.
            // It still needs filling: an unterminated block is not a legal
            // function.
            self.builder.ins().jump(merge, &[]);
        }

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        if all_returned {
            self.return_zero();
        }
        all_returned
    }

    /// Copy a matched variant's payload into the slots its pattern bound.
    fn bind_payload(
        &mut self,
        def: DefId,
        args: &[Type],
        variant: u32,
        arm: &Arm,
        values: &[Value],
    ) {
        let (offset, widths) = self.variant_layout(def, args, variant);
        let mut at = offset as usize;
        for (binding, width) in arm.bindings.iter().zip(widths) {
            if let Some(slot) = binding {
                let base = self.slot_base[slot.0 as usize];
                for index in 0..width {
                    let value = values[at + index as usize];
                    self.builder.def_var(Variable::from_u32(base + index), value);
                }
            }
            // A `_` binding still occupies its payload position; there is
            // simply nowhere to put the value.
            at += width as usize;
        }
    }

    /// An expression whose type has exactly one leaf.
    fn scalar(&mut self, expr: &Expr) -> Value {
        let values = self.expr(expr);
        debug_assert_eq!(values.len(), 1, "expected a scalar, got {} leaves", values.len());
        values[0]
    }

    /// Emit an expression as its leaf values, in field order.
    ///
    /// A scalar yields one value and a struct yields one per leaf field, which
    /// is why this returns a vector rather than a `Value`: there is no single
    /// register a struct lives in, because it does not live in memory either.
    fn expr(&mut self, expr: &Expr) -> Vec<Value> {
        match expr {
            Expr::Int(v) => vec![self.builder.ins().iconst(types::I64, *v)],
            Expr::Bool(v) => vec![self.builder.ins().iconst(types::I8, i64::from(*v))],
            Expr::Load(slot) => {
                let base = self.slot_base[slot.0 as usize];
                let count =
                    leaf_count(&self.func.slots[slot.0 as usize], self.program, self.pointer);
                (0..count)
                    .map(|offset| self.builder.use_var(Variable::from_u32(base + offset)))
                    .collect()
            }
            Expr::Struct { fields, .. } => {
                fields.iter().flat_map(|field| self.expr(field)).collect()
            }
            Expr::Field { base, def, args, index } => {
                let values = self.expr(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field access on an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let len = leaf_count(
                    &fields[*index as usize].1.substitute(args, &[]),
                    self.program,
                    self.pointer,
                );
                values[start as usize..(start + len) as usize].to_vec()
            }
            // The same field arithmetic as `Expr::Field`, except the leaves
            // are loaded out of the buffer the reference points at rather
            // than picked out of leaves already in registers.
            Expr::FieldRef { base, def, args, index } => {
                let address = self.scalar(base);
                let TypeInfo::Struct { fields, .. } = self.program.type_info(*def) else {
                    unreachable!("a field access on an enum should have been refused");
                };
                let start: u32 = fields[..*index as usize]
                    .iter()
                    .map(|(_, ty)| {
                        leaf_count(&ty.substitute(args, &[]), self.program, self.pointer)
                    })
                    .sum();
                let kinds = leaves(
                    &fields[*index as usize].1.substitute(args, &[]),
                    self.program,
                    self.pointer,
                );
                let offset = start as i32 * RETURN_SLOT_STRIDE;
                kinds
                    .iter()
                    .enumerate()
                    .map(|(i, kind)| {
                        let at = offset + i as i32 * RETURN_SLOT_STRIDE;
                        self.builder.ins().load(*kind, MemFlags::trusted(), address, at)
                    })
                    .collect()
            }
            Expr::Enum { def, args, variant, payload } => {
                let whole = Type::Named(*def, args.clone());
                let (offset, widths) = self.variant_layout(*def, args, *variant);
                let total = leaf_count(&whole, self.program, self.pointer);
                let payload: Vec<Vec<Value>> = payload.iter().map(|e| self.expr(e)).collect();
                let all = leaves(&whole, self.program, self.pointer);

                // The tag, then every variant's leaves. This variant's are the
                // values just computed; the rest are zeroed, because a value
                // that is not this variant is not readable without matching on
                // the tag first.
                let mut out = Vec::with_capacity(total as usize);
                out.push(self.builder.ins().iconst(types::I64, i64::from(*variant)));
                for index in 1..total {
                    out.push(self.builder.ins().iconst(all[index as usize], 0));
                }
                let mut at = offset as usize;
                for (values, width) in payload.into_iter().zip(widths) {
                    debug_assert_eq!(values.len(), width as usize);
                    for value in values {
                        out[at] = value;
                        at += 1;
                    }
                }
                out
            }
            Expr::Neg(inner) => {
                let v = self.scalar(inner);
                vec![self.builder.ins().ineg(v)]
            }
            Expr::Not(inner) => {
                // A `bool` is 0 or 1, so flipping the low bit is the negation.
                let v = self.scalar(inner);
                vec![self.builder.ins().bxor_imm(v, 1)]
            }
            Expr::Bin { op, lhs, rhs } if op.is_short_circuit() => {
                vec![self.short_circuit(*op, lhs, rhs)]
            }
            Expr::Bin { op, lhs, rhs } => {
                let a = self.scalar(lhs);
                let b = self.scalar(rhs);
                vec![self.binary(*op, a, b)]
            }
            Expr::Call { callee, args } => {
                let args: Vec<Value> = args.iter().flat_map(|a| self.expr(a)).collect();
                match callee {
                    Callee::Fn(id) => {
                        let callee = &self.program.funcs[id.0 as usize];
                        let ret = callee.ret.clone();
                        let f = self
                            .module
                            .declare_func_in_func(self.declared[id.0 as usize], self.builder.func);

                        if !returns_indirectly(&ret, self.program, self.pointer) {
                            let call = self.builder.ins().call(f, &args);
                            return self.builder.inst_results(call).to_vec();
                        }

                        // Too wide for registers: hand the callee somewhere to
                        // put it, then read it back.
                        let buffer = self.return_buffer(&ret);
                        let mut with_buffer = Vec::with_capacity(args.len() + 1);
                        with_buffer.push(buffer);
                        with_buffer.extend(args);
                        self.builder.ins().call(f, &with_buffer);
                        let kinds = leaves(&ret, self.program, self.pointer);
                        self.load_leaves(buffer, &kinds)
                    }
                    Callee::Builtin(Builtin::PutChar) => {
                        let f = self.module.declare_func_in_func(self.putchar, self.builder.func);
                        let arg = self.builder.ins().ireduce(types::I32, args[0]);
                        let call = self.builder.ins().call(f, &[arg]);
                        let result = self.builder.inst_results(call)[0];
                        vec![self.builder.ins().sextend(types::I64, result)]
                    }
                }
            }
        }
    }

    /// `&&` and `||`, which are control flow rather than instructions: the
    /// right operand must not be evaluated when the left already decides the
    /// answer.
    ///
    /// The result travels in a variable rather than a block parameter, so this
    /// reuses the same SSA construction the slots already use.
    fn short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Value {
        let result = self.temporary(types::I8);

        let rhs_block = self.builder.create_block();
        let merge = self.builder.create_block();

        let a = self.scalar(lhs);
        // Short-circuiting means the answer is the left operand itself.
        self.builder.def_var(result, a);
        match op {
            BinOp::And => self.builder.ins().brif(a, rhs_block, &[], merge, &[]),
            BinOp::Or => self.builder.ins().brif(a, merge, &[], rhs_block, &[]),
            other => unreachable!("`{other:?}` does not short-circuit"),
        };

        self.builder.switch_to_block(rhs_block);
        self.builder.seal_block(rhs_block);
        let b = self.scalar(rhs);
        self.builder.def_var(result, b);
        self.builder.ins().jump(merge, &[]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.builder.use_var(result)
    }

    fn binary(&mut self, op: BinOp, a: Value, b: Value) -> Value {
        let cc = match op {
            BinOp::Add => return self.builder.ins().iadd(a, b),
            BinOp::Sub => return self.builder.ins().isub(a, b),
            BinOp::Mul => return self.builder.ins().imul(a, b),
            // Cranelift's `sdiv`/`srem` trap on a zero divisor and on
            // `int::MIN / -1`. A trap is defined behaviour; C's answer here is
            // not, which is the difference the language exists to make (#1).
            BinOp::Div => return self.builder.ins().sdiv(a, b),
            BinOp::Rem => return self.builder.ins().srem(a, b),
            BinOp::Eq => IntCC::Equal,
            BinOp::Ne => IntCC::NotEqual,
            BinOp::Lt => IntCC::SignedLessThan,
            BinOp::Le => IntCC::SignedLessThanOrEqual,
            BinOp::Gt => IntCC::SignedGreaterThan,
            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
            other => unreachable!("`{other:?}` is not an instruction"),
        };
        // `icmp` yields an `i8` holding 0 or 1, which is exactly a `bool`.
        // M0 widened this to `i64`; nothing needs widening now.
        self.builder.ins().icmp(cc, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_sys_ir::lower;
    use lex_sys_syntax::parse;
    use object::{Object, ObjectSymbol, SymbolKind};

    const SOURCE: &str = "fn shout() -> int { return putchar(33); } \
                          fn main() -> int { return shout(); }";

    /// The two targets to check: this host's architecture, once per binary
    /// format, paired with the symbol prefix that format calls for.
    ///
    /// Cranelift compiles in only the host's backend, so the architecture has
    /// to be this host's — but the *format* need not be, which is what makes
    /// Mach-O's conventions testable from Linux and ELF's from darwin.
    fn targets() -> [(String, &'static str); 2] {
        let arch = host_triple().architecture.to_string();
        [(format!("{arch}-unknown-linux-gnu"), ""), (format!("{arch}-apple-darwin"), "_")]
    }

    /// Compile for a target and read back the object's symbol table.
    fn symbols(triple: &str) -> Vec<(String, bool)> {
        let ast = parse(SOURCE).expect("should parse");
        let program = lower(&ast).expect("should lower");
        let bytes = compile_object_for(&program, "main", triple.parse().expect("a valid triple"))
            .expect("should compile");
        let file = object::File::parse(&*bytes).expect("a readable object file");
        // Text symbols are the functions; an undefined import reads back as
        // `Unknown`, and section and file symbols are neither.
        let mut names: Vec<(String, bool)> = file
            .symbols()
            .filter(|s| matches!(s.kind(), SymbolKind::Text | SymbolKind::Unknown))
            .filter_map(|s| s.name().ok().map(|n| (n.to_owned(), s.is_global())))
            .filter(|(name, _)| !name.is_empty())
            .collect();
        names.sort();
        names
    }

    fn names(triple: &str) -> Vec<String> {
        symbols(triple).into_iter().map(|(n, _)| n).collect()
    }

    /// A symbol is spelled as it was declared, plus whatever the platform adds
    /// — and nothing more.
    ///
    /// `object` picks a `Mangling` from the binary format when the object is
    /// created and applies the Mach-O leading underscore itself, without being
    /// asked. Prefixing here as well produced `__main`, and the darwin linker,
    /// looking for `_main`, could not resolve it. Linux never sees this, which
    /// is exactly why the assertion covers both formats from any host.
    #[test]
    fn symbols_are_spelled_for_their_platform_and_prefixed_once() {
        for (triple, prefix) in targets() {
            let names = names(&triple);
            for base in ["main", "lexs_main", "lexs_shout", "putchar"] {
                let expected = format!("{prefix}{base}");
                assert!(
                    names.contains(&expected),
                    "{triple} should define `{expected}`, got {names:?}"
                );
            }
            assert!(
                !names.iter().any(|n| n.starts_with("__")),
                "{triple}: a doubly-prefixed symbol is an unresolvable link: {names:?}"
            );
        }
    }

    /// `main` is the only symbol the linker may bind from outside. Everything
    /// the program defines is local and carries the `lexs_` prefix, so a
    /// lex-sys function called `write` or `exit` cannot collide with libc's.
    /// A borrowing program reaches real instruction selection on every target
    /// we ship, not just the host's.
    ///
    /// `borrow` is the first thing in the language that needs an address:
    /// `stack_addr` plus loads through a pointer, where a pointer's width is
    /// the target's rather than a constant. Emitting for both formats is the
    /// cheapest way to find out that the layout code disagrees with one.
    ///
    /// Only the host's architecture is reachable here, because Cranelift
    /// builds one backend by default; aarch64 emission for this program was
    /// checked by hand with `cranelift-codegen`'s `arm64` feature turned on,
    /// and CI runs the whole suite natively on darwin-aarch64 anyway.
    #[test]
    fn a_borrow_lowers_on_every_target() {
        const BORROWING: &str = "\
            struct Wide { a: int, b: bool, c: int } \
            fn look[&r](w: &r Wide) -> int { return w.a + w.c; } \
            fn main() -> int { let w = Wide { a: 1, b: true, c: 2 }; \
            borrow w as &r in { return look(r) - 3; } }";
        for (triple, _) in targets() {
            let ast = parse(BORROWING).expect("should parse");
            let program = lower(&ast).expect("should lower");
            let triple: Triple = triple.parse().expect("a valid triple");
            compile_object_for(&program, "main", triple.clone())
                .unwrap_or_else(|e| panic!("`{triple}` should emit: {e}"));
        }
    }

    #[test]
    fn only_the_entry_point_is_global() {
        for (triple, _) in targets() {
            let triple = triple.as_str();
            for (name, global) in symbols(triple) {
                if name.contains("lexs_") {
                    assert!(!global, "{triple}: `{name}` should be local");
                }
            }
            let globals: Vec<String> = symbols(triple)
                .into_iter()
                .filter(|(name, global)| *global && !name.contains("putchar"))
                .map(|(name, _)| name)
                .collect();
            assert_eq!(globals.len(), 1, "{triple}: exactly one exported symbol, got {globals:?}");
            assert!(globals[0].ends_with("main"), "{triple}: {globals:?}");
        }
    }
}
