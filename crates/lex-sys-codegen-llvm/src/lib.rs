//! A second backend (`docs/llvm-backend.md`): textual LLVM IR, shelled out
//! to `clang` for both compiling and linking's object step -- the same "no
//! C++ build dependency" reasoning `lex-sys-codegen`'s own header already
//! used to pick Cranelift for M0, aimed at `clang` instead of
//! `cranelift-codegen` (§2 there).
//!
//! **Five slices in** (§5, each corrected or extended in the PR that
//! built it): function declarations and calls, `World`/capability
//! erasure, `putchar` and process exit (`tests/accept/llvm_smoke.ls`);
//! checked arithmetic, every trapping `BinOp` plus the bitwise operators
//! (`tests/accept/llvm_arith.ls`); control flow, `if`/`while` and the
//! two short-circuit operators, needing no `phi` because every local
//! here is already memory (`tests/accept/llvm_control.ls`); slices and
//! strings, `Expr::Bytes`/`Expr::Len`/`Expr::Index` (what finally lowers
//! `examples/hello.ls` -- the program §5 originally, and wrongly, named
//! as the first slice's own target); and structs and enums, `Expr::
//! Struct`/`Expr::Enum`/`Stmt::Match` -- a struct's leaves are just its
//! fields' leaves concatenated, an enum's are a tag followed by *every*
//! variant's payload (`tests/accept/enums.ls`).
//!
//! `docs/llvm-backend.md` §7.3's own gap inventory, first-named row
//! closed: `wrapping_add`/`wrapping_sub`/`wrapping_mul` are LLVM's own
//! `add`/`sub`/`mul`, already two's-complement wraparound with no
//! `nsw`/`nuw` requested, so unlike `binop`'s checked forms these need no
//! overflow check at all.
//!
//! §7.5 closed the next-named row: `Stmt::Region`/`Expr::AllocSlice` --
//! one `malloc` in, one `free` out, a bump pointer kept in two
//! `ptr`-typed `alloca` cells rather than in an SSA value, the same
//! arena `lex-sys-codegen`'s own `body/memory.rs` builds -- plus
//! `byte_of` and `Expr::Not`, the two smaller gaps actually standing
//! between this and `sieve`/`scan` building.
//!
//! §7.7 closed heap boxing -- `Expr::BoxedSlice`/`Expr::Contents`/
//! `Expr::UnboxedSlice`, `boxed_slice` sharing `alloc_slice`'s own
//! `slice_bytes` sizing and reaching for `malloc` instead of `bump` --
//! and, found underneath it, a second gap nothing had tried to lift
//! since the first slice: a function could not return more than one
//! leaf. `struct_ty`/`pack_struct` close it in general, not just for a
//! boxed slice's two leaves, the same shape `checked_arith`'s own
//! `{i64, i1}` intrinsic reads already are.
//!
//! §7.9 closed `getchar` (the mirror of `putchar`) and, found sitting
//! in front of `revcomp.ls`, `Expr::Subslice` (`s[a..b]`) -- the same
//! two bounds checks `element_address` already makes, over a range
//! rather than one element. `revcomp.ls` itself needs a third,
//! materially bigger gap past both: `Place::Field`/`Place::Deref`,
//! named since §5 and not closed here. Every `benches/` program behind
//! only the gaps closed so far now builds on `--backend llvm`.
//!
//! Still refused: matching *through* a reference (only an owned
//! scrutinee's tag and payload are read directly; `docs/reading-
//! references.md`'s address-only binding mode has no counterpart here
//! yet), `Place::Field`/`Place::Deref` (writing through a reference
//! needs pointer arithmetic into a referent this backend has not built
//! -- `revcomp.ls`'s own boundary now), bare `Expr::Alloc`/`Expr::
//! Boxed`/`Expr::Unboxed` (a single-value allocation, arena or heap --
//! nothing in `benches/` asks for one), `arg_count`, `Type::Float`,
//! `Ffi`/`extern fn`, `Net`, and every other `Builtin` beyond
//! `PutChar`/`GetChar`/`Split`/`Release`/`Narrow`/`IntOf`/`ByteOf`/
//! `WrappingAdd`/`WrappingSub`/`WrappingMul`.
//!
//! This backend is intentionally partial. Everything it does not yet lower
//! is refused with a [`CodegenError`], never a panic: unlike
//! `lex-sys-codegen`, whose `unreachable!`s state an invariant the checker
//! already guarantees, an unsupported node here is an ordinary gap in an
//! opt-in, unfinished backend, and a program hitting one deserves the same
//! located, non-crashing refusal `docs/internal-errors.md` promises
//! everywhere else.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use lex_sys_codegen::CodegenError;
use lex_sys_ir::Program;
use target_lexicon::Triple;

mod emit;

/// Compile a program to the bytes of a native object file for the host.
pub fn compile_object(program: &Program, entry: &str) -> Result<Vec<u8>, CodegenError> {
    compile_object_for(program, entry, lex_sys_codegen::host_triple())
}

/// Compile a program to the bytes of an object file for an explicit target.
///
/// Emits a `.ll` text module, then shells out to `clang -c` to turn it into
/// an object file (`docs/llvm-backend.md` §3.1 measured that `clang` alone
/// is enough -- no `llc`, no `opt`, and no new CI tooling beyond the `cc`
/// step `lex-sys-codegen`'s own `link` already depends on).
pub fn compile_object_for(
    program: &Program,
    entry: &str,
    triple: Triple,
) -> Result<Vec<u8>, CodegenError> {
    let text = emit::emit_module(program, entry, &triple)
        .map_err(|(function, message)| CodegenError { function, message })?;
    run_clang(&text, &triple)
}

/// A counter, not a process id: two calls in the same test process (as the
/// differential suite makes) must not collide on one temporary file.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// `-O2`, always: `mem2reg` -- the pass every leaf's `alloca` (`emit.rs`'s
/// own header) depends on to reach a register at all -- is **not** run at
/// `clang`'s default `-O0`. `docs/llvm-backend.md` §5 called it "mandatory,"
/// which was true of the *design* (`clang` needs no help from this crate to
/// promote memory to SSA) but false of the *invocation* this function used
/// to make: measured without `-O2`, `sum_checked.ls`'s loop compiles to nine
/// stack loads and stores per iteration, all of them live past `-O0`. `-O2`
/// is not an enhancement bolted on after the fact; it is what makes the
/// header's own claim true (`docs/llvm-backend.md` §7 corrects it in place).
fn run_clang(module: &str, triple: &Triple) -> Result<Vec<u8>, CodegenError> {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir();
    let stem = format!("lex-sys-llvm-{}-{id}", std::process::id());
    let ll_path = dir.join(format!("{stem}.ll"));
    let obj_path = dir.join(format!("{stem}.o"));

    std::fs::write(&ll_path, module).map_err(|e| CodegenError {
        function: None,
        message: format!("cannot write `{}`: {e}", ll_path.display()),
    })?;

    let result = (|| -> Result<Vec<u8>, CodegenError> {
        let cc = std::env::var("CLANG").unwrap_or_else(|_| "clang".to_owned());
        let status = Command::new(&cc)
            .arg("-c")
            .arg("-O2")
            .arg("-target")
            .arg(triple.to_string())
            .arg(&ll_path)
            .arg("-o")
            .arg(&obj_path)
            .output()
            .map_err(|e| CodegenError {
                function: None,
                message: format!("cannot run `{cc}`: {e}"),
            })?;
        if !status.status.success() {
            return Err(CodegenError {
                function: None,
                message: format!(
                    "`{cc} -c` refused the emitted module:\n{}",
                    String::from_utf8_lossy(&status.stderr)
                ),
            });
        }
        std::fs::read(&obj_path).map_err(|e| CodegenError {
            function: None,
            message: format!("cannot read `{}`: {e}", obj_path.display()),
        })
    })();

    let _ = std::fs::remove_file(&ll_path);
    let _ = std::fs::remove_file(&obj_path);
    result
}

#[cfg(test)]
mod tests;
