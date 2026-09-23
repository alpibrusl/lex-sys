//! One function's body. `BodyEmitter` is one type, and its `impl` is
//! split by concern across this module's children, which see its
//! private fields.

use crate::*;

mod control;
mod expr;
mod memory;

pub(crate) struct BodyEmitter<'a, 'f> {
    pub(crate) builder: FunctionBuilder<'f>,
    pub(crate) module: &'a mut ObjectModule,
    pub(crate) declared: &'a [FuncId],
    /// The foreign imports, in `Program::externs` order.
    pub(crate) foreign: &'a [FuncId],
    pub(crate) console: Console,
    pub(crate) func: &'a Func,
    pub(crate) program: &'a Program,
    /// The buffer this function writes its result into, when its return type
    /// is too wide for registers.
    pub(crate) return_pointer: Option<Value>,
    pub(crate) pointer: types::Type,
    /// Where each slot's leaves begin among the function's variables.
    pub(crate) slot_base: Vec<u32>,
    /// How many string literals have been emitted so far, so each gets its
    /// own symbol. Two occurrences of the same text get two data objects:
    /// interning is an optimisation that changes whether they share an
    /// address, which is observable, so `docs/strings.md` §8 keeps it open
    /// until there is a rule for it.
    pub(crate) literals: u32,
    /// Each arena's base pointer and bump pointer, **indexed by the arena
    /// number** `Stmt::Region` carries, and `None` where that arena is not
    /// open here. Variables rather than values because a `region` inside a
    /// loop opens a fresh arena on every iteration.
    ///
    /// Indexed rather than stacked, which it was until two *sibling*
    /// `region` blocks crashed the compiler. Arena numbers are handed out
    /// in the order the lowering meets the blocks, so siblings get 0 and
    /// 1 — but the second opens after the first has closed, and a stack
    /// had length 0 when it wanted index 1. No program in the repository
    /// had two regions side by side, so the assertion that documented the
    /// assumption held for a year and was wrong the whole time.
    pub(crate) arenas: Vec<Option<(Variable, Variable)>>,
    /// Slot leaves occupy the variables below this; temporaries the backend
    /// needs for its own purposes are numbered from here.
    pub(crate) next_var: u32,
}

impl<'a, 'f> BodyEmitter<'a, 'f> {
    pub(crate) fn new(
        builder: FunctionBuilder<'f>,
        module: &'a mut ObjectModule,
        declared: &'a [FuncId],
        foreign: &'a [FuncId],
        console: Console,
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
            foreign,
            console,
            func,
            program,
            return_pointer: None,
            pointer,
            slot_base,
            literals: 0,
            arenas: Vec::new(),
            next_var,
        }
    }

    /// Write leaf values into an indirect return buffer.
    pub(crate) fn store_leaves(&mut self, address: Value, values: &[Value]) {
        for (index, value) in values.iter().enumerate() {
            let offset = index as i32 * RETURN_SLOT_STRIDE;
            self.builder.ins().store(MemFlags::trusted(), *value, address, offset);
        }
    }

    /// Read leaf values back out of one.
    pub(crate) fn load_leaves(&mut self, address: Value, kinds: &[types::Type]) -> Vec<Value> {
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
    pub(crate) fn return_buffer(&mut self, ty: &Type) -> Value {
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
    pub(crate) fn temporary(&mut self, ty: types::Type) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        self.builder.declare_var(var, ty);
        var
    }

    pub(crate) fn emit_func(&mut self, func: &Func) {
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
                let zero = self.zero(leaf);
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

    /// A zero of a leaf's machine type.
    ///
    /// `iconst` is an *integer* instruction, and its verifier rejects a
    /// floating control type -- so a `float` leaf needs `f64const`. That
    /// is the whole of the difference, and it is the one place adding a
    /// second machine class to the language was not free
    /// (`docs/floating-point.md` §1).
    pub(crate) fn zero(&mut self, leaf: types::Type) -> Value {
        if leaf == types::F64 {
            self.builder.ins().f64const(0.0)
        } else {
            self.builder.ins().iconst(leaf, 0)
        }
    }

    /// Return a zero of the function's return type, however many leaves it has.
    pub(crate) fn return_zero(&mut self) {
        let zeros: Vec<Value> = leaves(&self.func.ret, self.program, self.pointer)
            .into_iter()
            .map(|leaf| self.zero(leaf))
            .collect();
        self.emit_return(zeros);
    }

    /// Hand back a result, in registers or through the caller's buffer.
    pub(crate) fn emit_return(&mut self, values: Vec<Value>) {
        // Leaving every arena this `return` jumps out of, innermost first.
        // The returned value cannot point into one — §6's occurs-check is
        // what guarantees that — so releasing here is releasing memory
        // nothing can still reach.
        for index in (0..self.arenas.len()).rev() {
            let Some((base_var, _)) = self.arenas[index] else { continue };
            let held = self.builder.use_var(base_var);
            self.free(held);
        }
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
    pub(crate) fn stmts(&mut self, stmts: &[Stmt]) -> bool {
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
                Stmt::Region { arena, body } => {
                    if self.region_stmt(*arena, body) {
                        return true;
                    }
                }
                Stmt::Borrow { referent, reference, unique, body } => {
                    if self.borrow_stmt(*referent, *reference, *unique, body) {
                        return true;
                    }
                }
                Stmt::Match { scrutinee, def, args, arms, by_reference } => {
                    if self.match_stmt(scrutinee, *def, args, arms, *by_reference) {
                        return true;
                    }
                }
            }
        }
        false
    }
}
