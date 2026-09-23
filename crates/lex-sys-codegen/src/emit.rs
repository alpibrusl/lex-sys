//! The module: declarations, data, each function's body, and the C
//! entry point that hands `main` its `World`.

use crate::*;

pub(crate) struct Emitter<'a> {
    pub(crate) module: ObjectModule,
    pub(crate) program: &'a Program,
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
    pub(crate) fn emit(&mut self, entry: &str) -> Result<(), CodegenError> {
        let call_conv = self.module.isa().default_call_conv();
        let pointer = self.module.isa().pointer_type();
        // Bound once: the body emitter borrows the module mutably, so the
        // program has to be reached through a separate binding.
        let program = self.program;

        // Every `static`'s bytes, laid out once (`docs/compile-time-data.md`
        // §2). Defined before any body is emitted, so a reference from a
        // function finds a symbol that already exists — and defined once
        // for the program rather than once per reader, because a table is
        // not a greeting.
        for data in &self.program.statics {
            let stride = match data.element {
                Type::Byte => 1usize,
                _ => leaf_count(&data.element, program, pointer) as usize * 8,
            };
            // An empty `static` still needs an address, for the reason a
            // zero-length literal does: a slice is a pointer and a length,
            // and the pointer has to be *some*thing.
            let mut bytes = vec![0u8; (data.values.len() * stride).max(1)];
            for (i, value) in data.values.iter().enumerate() {
                let at = i * stride;
                bytes[at..at + stride].copy_from_slice(&value.to_le_bytes()[..stride]);
            }
            let mut description = DataDescription::new();
            description.define(bytes.into_boxed_slice());
            let name = format!("{PREFIX}static_{}", data.name);
            let id = self
                .module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|e| CodegenError::plain(e.to_string()))?;
            self.module
                .define_data(id, &description)
                .map_err(|e| CodegenError::plain(e.to_string()))?;
        }

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
                .map_err(|e| CodegenError::plain(e.to_string()))?;
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
            .declare_function(
                Builtin::PutChar.symbol().expect("putchar reaches libc"),
                Linkage::Import,
                &putchar_sig,
            )
            .map_err(|e| CodegenError::plain(e.to_string()))?;

        // `docs/standard-input.md` §3: the mirror. `int getchar(void)` --
        // no parameter, and the same `i32` result that widens at the edge,
        // which is what carries the `-1` back as a `-1` rather than as a
        // very large unsigned number.
        let mut getchar_sig = self.module.make_signature();
        getchar_sig.call_conv = call_conv;
        getchar_sig.returns.push(AbiParam::new(types::I32));
        let getchar = self
            .module
            .declare_function(
                Builtin::GetChar.symbol().expect("getchar reaches libc"),
                Linkage::Import,
                &getchar_sig,
            )
            .map_err(|e| CodegenError::plain(e.to_string()))?;

        // `size_t fwrite(const void *, size_t, size_t, FILE *)` — four
        // pointer-or-size arguments and a count back.
        let mut fwrite_sig = self.module.make_signature();
        fwrite_sig.call_conv = call_conv;
        fwrite_sig.params.push(AbiParam::new(pointer));
        fwrite_sig.params.push(AbiParam::new(pointer));
        fwrite_sig.params.push(AbiParam::new(pointer));
        fwrite_sig.params.push(AbiParam::new(pointer));
        fwrite_sig.returns.push(AbiParam::new(pointer));
        let fwrite = self
            .module
            .declare_function(
                Builtin::Write.symbol().expect("write_bytes reaches libc"),
                Linkage::Import,
                &fwrite_sig,
            )
            .map_err(|e| CodegenError::plain(e.to_string()))?;

        let stdout_symbol = match self.module.isa().triple().operating_system {
            target_lexicon::OperatingSystem::Darwin(_) => "__stdoutp",
            _ => "stdout",
        };
        let stdout = self
            .module
            .declare_data(stdout_symbol, Linkage::Import, true, false)
            .map_err(|e| CodegenError::plain(e.to_string()))?;

        let stderr_symbol = match self.module.isa().triple().operating_system {
            target_lexicon::OperatingSystem::Darwin(_) => "__stderrp",
            _ => "stderr",
        };
        let stderr = self
            .module
            .declare_data(stderr_symbol, Linkage::Import, true, false)
            .map_err(|e| CodegenError::plain(e.to_string()))?;
        let console = Console { putchar, getchar, fwrite, stdout, stderr };

        // §8.4: a foreign function is an import under the symbol its
        // declaration named. Its capability parameters carry no data and so
        // never reach C; everything else crosses at lex-sys's own widths,
        // which is why the boundary admits `int` and `bool` and nothing that
        // would need a layout agreement neither side has made.
        let mut foreign: Vec<FuncId> = Vec::with_capacity(self.program.externs.len());
        for ext in &self.program.externs {
            let mut sig = self.module.make_signature();
            sig.call_conv = call_conv;
            for param in ext.params.iter().filter(|t| crosses_to_c(t)) {
                for leaf in leaves(param, self.program, pointer) {
                    sig.params.push(AbiParam::new(leaf));
                }
            }
            for leaf in leaves(&ext.ret, self.program, pointer) {
                sig.returns.push(AbiParam::new(leaf));
            }
            let id = self
                .module
                .declare_function(&ext.symbol, Linkage::Import, &sig)
                .map_err(|e| CodegenError::plain(e.to_string()))?;
            foreign.push(id);
        }

        let mut ctx = Context::new();
        let mut fb_ctx = FunctionBuilderContext::new();

        for (index, func) in self.program.funcs.iter().enumerate() {
            ctx.clear();
            ctx.func.signature =
                self.module.declarations().get_function_decl(declared[index]).signature.clone();

            // `docs/internal-errors.md` §4: every assertion below is an
            // invariant between the checker and this crate, and one that
            // breaks is the compiler's bug. Caught here, at the function,
            // so it is reported against the function it happened in
            // rather than as a bare panic. Nothing is resumed: the first
            // failure ends code generation for the whole program.
            let module = &mut self.module;
            let generated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                {
                    let builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
                    let mut body = BodyEmitter::new(
                        builder, module, &declared, &foreign, console, func, program,
                    );
                    body.emit_func(func);
                    body.builder.finalize();
                }
                module.define_function(declared[index], &mut ctx).map_err(|e| e.to_string())
            }));
            match generated {
                Ok(Ok(())) => {}
                Ok(Err(message)) => {
                    return Err(CodegenError { function: Some(index), message });
                }
                Err(payload) => {
                    return Err(CodegenError {
                        function: Some(index),
                        message: panic_text(payload.as_ref()),
                    });
                }
            }
        }

        let entry_id = self.program.find(entry).ok_or_else(|| {
            CodegenError::plain(format!("no function named `{entry}` to use as entry"))
        })?;
        self.emit_c_main(entry_id, &declared, &mut ctx, &mut fb_ctx)
    }

    /// Synthesise `int main(int argc, char **argv)`, which stashes what the
    /// runtime handed over, calls the lex-sys entry function and truncates
    /// its result to the platform's exit status.
    ///
    /// `docs/arguments.md` §3: the two globals are where `arg_count` and
    /// `arg` read from. They are written exactly once, before any lex-sys
    /// code runs, and never again — which is why a program cannot write
    /// through an argument and why the bytes it reads are the bytes it was
    /// started with.
    pub(crate) fn emit_c_main(
        &mut self,
        entry: IrFuncId,
        declared: &[FuncId],
        ctx: &mut Context,
        fb_ctx: &mut FunctionBuilderContext,
    ) -> Result<(), CodegenError> {
        let pointer = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.call_conv = self.module.isa().default_call_conv();
        sig.params.push(AbiParam::new(types::I32));
        sig.params.push(AbiParam::new(pointer));
        sig.returns.push(AbiParam::new(types::I32));
        let main = self
            .module
            .declare_function("main", Linkage::Export, &sig)
            .map_err(|e| CodegenError::plain(e.to_string()))?;

        // Defined here rather than on first use, because `main` is the one
        // function guaranteed to exist and the only one that can fill them.
        for name in [ARGC_GLOBAL, ARGV_GLOBAL] {
            let id = self
                .module
                .declare_data(name, Linkage::Local, true, false)
                .map_err(|e| CodegenError::plain(e.to_string()))?;
            let mut description = DataDescription::new();
            description.define_zeroinit(RETURN_SLOT_STRIDE as usize);
            self.module
                .define_data(id, &description)
                .map_err(|e| CodegenError::plain(e.to_string()))?;
        }

        ctx.clear();
        ctx.func.signature = sig;
        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, fb_ctx);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);

            let argc = builder.block_params(block)[0];
            let argv = builder.block_params(block)[1];
            let argc = builder.ins().sextend(types::I64, argc);
            for (name, value) in [(ARGC_GLOBAL, argc), (ARGV_GLOBAL, argv)] {
                let id = self
                    .module
                    .declare_data(name, Linkage::Local, true, false)
                    .map_err(|e| CodegenError::plain(e.to_string()))?;
                let global = self.module.declare_data_in_func(id, builder.func);
                let address = builder.ins().global_value(pointer, global);
                builder.ins().store(MemFlags::trusted(), value, address, 0);
            }

            let callee = self.module.declare_func_in_func(declared[entry.0 as usize], builder.func);
            let call = builder.ins().call(callee, &[]);
            let status = builder.inst_results(call)[0];
            let status = builder.ins().ireduce(types::I32, status);
            builder.ins().return_(&[status]);
            builder.finalize();
        }
        self.module.define_function(main, ctx).map_err(|e| CodegenError::plain(e.to_string()))?;
        Ok(())
    }
}
