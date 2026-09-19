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
use cranelift_codegen::ir::{AbiParam, InstBuilder, Value, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, isa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use lex_sys_ir::{
    BinOp, Builtin, Callee, Expr, Func, FuncId as IrFuncId, Program, Stmt, terminates,
};
use target_lexicon::Triple;

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

        // Declare every lex-sys function first: calls are resolved against
        // declarations, so definition order in the file never matters.
        let mut declared: Vec<FuncId> = Vec::with_capacity(self.program.funcs.len());
        for func in &self.program.funcs {
            let mut sig = self.module.make_signature();
            sig.call_conv = call_conv;
            for _ in 0..func.n_params {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
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
                    BodyEmitter { builder, module: &mut self.module, declared: &declared, putchar };
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
}

impl<'a, 'f> BodyEmitter<'a, 'f> {
    fn emit_func(&mut self, func: &Func) {
        let entry = self.builder.create_block();
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);
        self.builder.seal_block(entry);

        // Every slot is a Cranelift variable; the SSA builder turns them back
        // into values. Parameters take their incoming values, everything else
        // starts at zero so no path can observe an undefined slot.
        for slot in 0..func.n_slots {
            self.builder.declare_var(Variable::from_u32(slot), types::I64);
        }
        for slot in 0..func.n_params {
            let value = self.builder.block_params(entry)[slot as usize];
            self.builder.def_var(Variable::from_u32(slot), value);
        }
        for slot in func.n_params..func.n_slots {
            let zero = self.builder.ins().iconst(types::I64, 0);
            self.builder.def_var(Variable::from_u32(slot), zero);
        }

        let terminated = self.stmts(&func.body);
        if !terminated {
            // Unreachable in a well-formed program: lowering proved every path
            // returns. Emitted so the block is filled whatever happens.
            let zero = self.builder.ins().iconst(types::I64, 0);
            self.builder.ins().return_(&[zero]);
        }
    }

    /// Emit a statement list; returns whether control left via `return`.
    fn stmts(&mut self, stmts: &[Stmt]) -> bool {
        for stmt in stmts {
            match stmt {
                Stmt::Store { slot, value } => {
                    let value = self.expr(value);
                    self.builder.def_var(Variable::from_u32(slot.0), value);
                }
                Stmt::Eval(expr) => {
                    self.expr(expr);
                }
                Stmt::Return(expr) => {
                    let value = self.expr(expr);
                    self.builder.ins().return_(&[value]);
                    return true;
                }
                Stmt::If { cond, then_body, else_body } => {
                    if self.if_stmt(cond, then_body, else_body) {
                        return true;
                    }
                }
                Stmt::While { cond, body } => self.while_stmt(cond, body),
            }
        }
        false
    }

    fn if_stmt(&mut self, cond: &Expr, then_body: &[Stmt], else_body: &[Stmt]) -> bool {
        let cond = self.expr(cond);
        let then_block = self.builder.create_block();
        let else_block = self.builder.create_block();
        let merge = self.builder.create_block();

        // `brif` tests the condition for non-zero. M0 has no `bool`; M1's type
        // checker is what turns this convention into a type.
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
            // legal function, and this costs one instruction the linker drops.
            let zero = self.builder.ins().iconst(types::I64, 0);
            self.builder.ins().return_(&[zero]);
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
        let cond = self.expr(cond);
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

    fn expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Const(v) => self.builder.ins().iconst(types::I64, *v),
            Expr::Load(slot) => self.builder.use_var(Variable::from_u32(slot.0)),
            Expr::Neg(inner) => {
                let v = self.expr(inner);
                self.builder.ins().ineg(v)
            }
            Expr::Bin { op, lhs, rhs } => {
                let a = self.expr(lhs);
                let b = self.expr(rhs);
                self.binary(*op, a, b)
            }
            Expr::Call { callee, args } => {
                let args: Vec<Value> = args.iter().map(|a| self.expr(a)).collect();
                match callee {
                    Callee::Fn(id) => {
                        let f = self
                            .module
                            .declare_func_in_func(self.declared[id.0 as usize], self.builder.func);
                        let call = self.builder.ins().call(f, &args);
                        self.builder.inst_results(call)[0]
                    }
                    Callee::Builtin(Builtin::PutChar) => {
                        let f = self.module.declare_func_in_func(self.putchar, self.builder.func);
                        let arg = self.builder.ins().ireduce(types::I32, args[0]);
                        let call = self.builder.ins().call(f, &[arg]);
                        let result = self.builder.inst_results(call)[0];
                        self.builder.ins().sextend(types::I64, result)
                    }
                }
            }
        }
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
        };
        let flag = self.builder.ins().icmp(cc, a, b);
        // A comparison is an `int` that is 0 or 1 until M1 gives it a `bool`.
        self.builder.ins().uextend(types::I64, flag)
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

    /// Compile for a target and read back the object's symbol table.
    ///
    /// Cranelift compiles in only the host's backend, so the *architecture*
    /// here has to be this host's — but the *binary format* need not be, which
    /// is what makes the Mach-O conventions testable from Linux.
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
        for (triple, prefix) in [("x86_64-unknown-linux-gnu", ""), ("x86_64-apple-darwin", "_")] {
            let names = names(triple);
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
    #[test]
    fn only_the_entry_point_is_global() {
        for triple in ["x86_64-unknown-linux-gnu", "x86_64-apple-darwin"] {
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
