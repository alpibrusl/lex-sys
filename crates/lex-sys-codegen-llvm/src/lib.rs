//! A second backend (`docs/llvm-backend.md`): textual LLVM IR, shelled out
//! to `clang` for both compiling and linking's object step -- the same "no
//! C++ build dependency" reasoning `lex-sys-codegen`'s own header already
//! used to pick Cranelift for M0, aimed at `clang` instead of
//! `cranelift-codegen` (§2 there).
//!
//! **First slice** (§5, corrected in the PR that added this crate):
//! function declarations and calls, `World`/capability erasure, `putchar`,
//! and process exit. Checked arithmetic, bounds-checked indexing and
//! string-literal data are **not** in this slice -- `examples/hello.ls`
//! needs all three, so it is not this slice's target; see
//! `tests/accept/llvm_smoke.ls` for the program this slice actually
//! builds and runs.
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
