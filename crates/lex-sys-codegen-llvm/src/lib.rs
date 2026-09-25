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
//! rather than one element. `revcomp.ls` itself needed a third,
//! materially bigger gap past both: `Place::Field`/`Place::Deref`.
//!
//! §7.11 closed that gap, and the read-side cluster sitting behind the
//! same name: `Expr::Deref`/`Expr::FieldRef`/`Expr::FieldAddr` and
//! their tuple-shaped counterparts, plus `Expr::Tuple`/`Expr::
//! TupleField` -- one shared `field_offset`/`tuple_field_offset` helper
//! underneath all of them -- and, found while building `revcomp.ls`
//! against it, `write_bytes`/`write_err` (`fwrite` through `stdout`/
//! `stderr`, platform-symbol resolution mirroring `lex-sys-codegen`'s
//! own). `revcomp.ls` now builds, runs, and matches the Benchmarks
//! Game's own published output.
//!
//! §7.13 closed `arg_count`/`arg` -- `argc`/`argv` stashed once into
//! module-local storage by `@main`, mirroring `lex-sys-codegen`'s own
//! `emit_c_main`, `arg`'s bounds check the same `icmp uge` shape every
//! other indexing operation here already uses. Only one of the two
//! named targets built past it at the time: `fannkuch.ls`, checked
//! against a real argument and not only the no-argument fallback;
//! `binarytrees.ls` reached bare `Expr::Boxed` instead.
//!
//! §7.15 closed that too: single-value allocation, arena (`alloc`) or
//! heap (`box`/`unbox`), each one a smaller version of a primitive this
//! backend had already built for a slice's many elements --
//! `alloc_slice`'s own `bump`, `boxed_slice`'s own `malloc`-and-null-
//! check, both minus their fill loop. `binarytrees.ls` builds too now,
//! taking both of §7.13's named targets with it rather than only one.
//!
//! §7.17 closed `Type::Float` -- the structural gap underneath it,
//! `scalar_kind`, mattered more than the arithmetic itself: unlike
//! Cranelift's intrinsically-typed `Value`, this backend's `LValue` has
//! no type tag, so `binop`/`Expr::Neg` had no way to tell a `float`
//! operand from an `int` one before evaluating it. `scalar_kind`
//! answers that structurally, without evaluating anything, and every
//! float operation -- unchecked arithmetic, ordered `fcmp` comparisons
//! (`!=` the one unordered exception), `float_of`/`truncate` (three
//! explicit checks ahead of `fptosi`, which is poison rather than
//! trapping on the inputs that matter), `bits_of`'s NaN
//! canonicalisation, `sqrt`, and (found unrelated but closed alongside,
//! since nothing had built it either) both halves of `Expr::Neg` --
//! follows from it. `spectral.ls` and `fasta.ls`, `Type::Float`'s two
//! named targets, both build and match Cranelift exactly. Every
//! `benches/` program this document tracks now builds on `--backend
//! llvm`.
//!
//! §7.19 closed matching *through* a reference -- unlike every slice
//! before it, close to what it looked like on the surface, because
//! §7.11's `getelementptr`-address idiom and the fifth slice's
//! `variant_layout`/`bind_payload` had already built everything it
//! needed: a reference is one pointer leaf, so the scrutinee evaluates
//! the same way in both modes and only the tag read differs (one more
//! `load` by reference); `bind_payload` gained the address-only mirror
//! of its by-value half, `variant_layout`'s own offset scaled to bytes
//! via `getelementptr` rather than a loaded value. `tests/accept/
//! match_a_reference.ls` and `examples/tree.ls` (a three-field variant,
//! `_` discarding past two positions, folded into a multi-leaf struct
//! return) both build and match Cranelift exactly. Every gap this
//! document names a `benches/`/`tests/accept/` target for is now
//! closed.
//!
//! §7.20 closed the first two of `Net`'s four builtins: `listen`/
//! `accept`, neither of which takes a capability (the fd's authority
//! was already proved at `bind`), so both are ordinary fixed-signature
//! `libc` calls, no different in shape from `Sqrt`. This slice was
//! scoped directly rather than by a `benches/` program -- `Net` has
//! none -- and closes smallest-first: `connect`/`bind` and `Ffi`/
//! `extern fn` are still refused, and with them the only way to obtain
//! a *real* fd, so `listen`/`accept` are tested here against a
//! deliberately invalid one (`tests/accept/listen_accept_bad_fd.ls`).
//!
//! §7.21 closed `bind`: `socket`+`setsockopt(SO_REUSEADDR)`+`bind`
//! folded into one call, the same `struct sockaddr_in`
//! `lex-sys-codegen`'s own `bind` builds by hand. The one new shape:
//! `socket`/`bind` can each fail, returning `-1` rather than trapping
//! (only a bound mismatch traps, checked first), so this is the first
//! *expression* needing a value conditional on which of three runtime
//! paths ran -- a plain `alloca i64` result cell written in each
//! branch and loaded once at the merge label, following `if_stmt`'s
//! own "no `phi`" rule rather than introducing a new one.
//! `crates/lex-sys/tests/conformance/backends.rs`'s
//! `the_two_backends_bind_and_accept_a_real_connection` builds a
//! listener on each backend, connects a real `TcpStream` from the test
//! process, and checks both exit `0` -- the first Net-capable program
//! this backend has ever actually run, not a bad-fd stand-in.
//!
//! §7.22 closed `connect`, the last of `Net`'s four builtins -- genuinely
//! the larger remaining piece, as §7.21 predicted. `checked_host`
//! (mirroring `lex-sys-codegen`'s own function) is this backend's first
//! *loop* built for `Net`: copy the dialled name into a 256-byte stack
//! buffer while checking, byte by byte, that the prefix inside the
//! capability's bound matches, in the same "cursor in an `alloca i64`
//! cell, no `phi`" shape `body/memory.rs`'s arena-fill loop already
//! established. `connect` then builds a `struct addrinfo hints`,
//! resolves with `getaddrinfo`, patches the port the same big-endian way
//! `bind` does, and calls `socket`/`connect` -- three failure points
//! rather than `bind`'s two, funnelled into the same one-result-cell
//! shape. This slice's own session built an all-LLVM pair -- a listener
//! and a client, both `--backend llvm` -- talking over real loopback,
//! the first time two programs this backend built have ever talked to
//! each other; `backends.rs`'s `the_two_backends_connect_to_a_real_
//! listener` checks the same claim against a plain
//! `std::net::TcpListener` peer.
//!
//! `Net` is now fully built on `--backend llvm`: `listen`, `accept`,
//! `bind` and `connect` all lower.
//!
//! §7.23 closed `Ffi`/`extern fn`, named since §7.19 -- and found this
//! summary's own "only remaining gap" claim, repeated across §7.20-
//! §7.22, false: `Fs` had been unbuilt the whole time, unnamed here
//! because no slice had tried a real `Fs`-using program against this
//! backend until this one did (`examples/seek/`). Corrected, not
//! deleted -- `docs/llvm-backend.md` §7.23 has the finding in full.
//! `scalar_kind` (§7.17) needed a new arm for `Callee::Extern`, the same
//! shape `Callee::Fn`'s own already has; the call site itself reuses
//! `Callee::Fn`'s call-and-unpack logic, factored out as `emit_call`
//! once both needed it. Checked against `tests/accept/bytes_to_c.ls`
//! (a `&r [byte]`-crossing `write`, unmodified for this slice) and a
//! fresh `labs(-5) == 5` check on both backends.
//!
//! §7.24 closed `Fs`: `checked_path` is `checked_host`'s loop plus a
//! `..`-traversal refusal and a `/`-boundary check, reused by `file_op`
//! (`fs_read`/`fs_write`) and `open_file` (`open_read`); `read_file`
//! (`file_read`) and `errno` (a per-thread libc accessor, one more
//! `__errno_location`/`__error` platform split) round it out.
//! `leaves_into` gained a `PRELUDE_FILE` arm -- a handle is one `i64`
//! leaf, the same special case `Box` already has.
//!
//! Checking it against real programs found two things. `read`/`write`,
//! declared unconditionally for `Fs`'s own use, broke `tests/accept/
//! bytes_to_c.ls` -- a program §7.23 had just proven working, not one
//! that had never worked, so unlike `socket`/`bind`/`connect`'s already-
//! documented exposure this one is fixed: both are now declared only
//! when `program.externs` does not already claim the symbol. And
//! `compare` had always hardcoded `icmp {cc} i64` regardless of its
//! operands' real type -- silently correct for `int`, silently
//! **ill-typed** for `byte`/`bool` -- caught only once `examples/cut/`/
//! `examples/seek/` (both reaching `Fs` only incidentally, through
//! `std.flags`) exercised `std.bytes.find`'s raw `byte` comparison,
//! which nothing in eighteen prior slices had. Fixed by threading
//! `binop`'s already-computed `lhs_kind` through to `compare`.
//!
//! The boundary fixture moved a seventh time, from `tests/accept/
//! bytes_to_c.ls` (inside since §7.23) to `tests/accept/static_data.ls`,
//! refusing on `Expr::Static` -- `docs/compile-time-data.md`'s whole
//! feature, found unbuilt the same way `Fs` was, by checking rather than
//! assuming this backend's `Expr` match was exhaustive.
//!
//! §7.25 closed `Expr::Static` and, found next to it, `Expr::BitNot` --
//! `~x` on `docs/bitwise.md` §1's `int`. `body/expr.rs`'s own `expr`
//! match had no arm for `BitNot` at all, and nothing here had noticed:
//! `tests/accept/bitwise.ls`'s one use, `~0`, is a literal the checker
//! folds away before codegen ever runs, so the gap was silently
//! unreachable rather than silently wrong, the same shape `compare`'s
//! own bug (§7.24) was not -- found this time by checking that the
//! `expr` match was *actually* exhaustive rather than assuming it once
//! more, the question §7.24's own finding should have raised. `Static`
//! itself needed two things: `emit_module` lays out each `Program::
//! statics` entry as one `private unnamed_addr constant [N x i8]`
//! global, packed at `stride_of`'s own byte-or-eight-byte stride so the
//! bytes match how every other slice in this backend is laid out; and
//! `Expr::Static` is then only a reference to that already-declared
//! symbol, the same shape a string literal's *value* is once
//! `bytes_lit` has declared its own global, minus the per-occurrence
//! declaration since a static's global is defined once for the whole
//! program. `BitNot` is `Not`'s own `xor`, at `i64` and `-1` rather than
//! `i8` and `1`, since the two operators disagree on nothing else.
//!
//! Both closures left `expr`'s match with no unmatched `Expr` variant --
//! so the wildcard arm that used to say "not part of the LLVM backend
//! yet" came out, and `rustc` itself now refuses to build this crate if
//! a future `Expr` variant goes unhandled, which is a stronger promise
//! than any test in this crate could make on its own. The same check
//! run over `Callee::Builtin`'s variants found nothing left either:
//! every one reaches an arm, and `FsRead`/`FsWrite`/`OpenRead` never
//! reach `Callee::Builtin` at all, lowered as their own `Expr::FileOp`/
//! `Expr::OpenFile` nodes the same way `Connect`/`Bind` are. Checked
//! against real programs rather than only against the enums: every
//! fixture in `tests/accept/` and every program in `examples/` was
//! built against `--backend llvm` in this slice's own session, and all
//! of them succeed. `tests/accept/static_data.ls` matches Cranelift byte
//! for byte (`backends.rs`'s `the_two_backends_agree_on_static_data`);
//! `tests/accept/bitwise.ls` does too, now checked against this backend
//! for the first time (`the_two_backends_agree_on_bitwise`) -- and
//! because its own `~0` cannot exercise a genuinely unfoldable operand,
//! `bitnot_flips_every_bit_not_just_the_low_one`
//! (`crates/lex-sys-codegen-llvm/src/tests.rs`) checks `~x` on
//! `putchar`'s own runtime echo instead.
//!
//! One friction remains, and it is not a gap this backend has left
//! unbuilt: `examples/collect/`/`fetch/`/`report/`/`serve/` each declare
//! their own `extern fn socket`, colliding with the fixed-width
//! `@socket` (and `bind`/`connect`/`listen`/`accept`/`setsockopt`/
//! `close`) this backend declares unconditionally for `Net`'s own use --
//! `clang` correctly refuses to link the disagreement rather than
//! silently miscompiling it. Not a new finding: the same exposure
//! `docs/ROADMAP.md`'s #92 entry already recorded on Cranelift for
//! `close`, left unfixed there and here for the same reason a program
//! declaring the real libc signature for itself does not need this
//! backend's own internal one. `a_program_outside_this_backend_is_
//! refused_not_panicked`/`a_program_outside_this_backend_is_refused_
//! through_the_cli` now check that collision directly, in place of the
//! unbuilt-`Expr` refusal both used to name -- there being no unbuilt
//! `Expr` left to name.
//!
//! This backend is intentionally partial in the sense that mattered when
//! this module header was first written -- real programs it cannot yet
//! run -- and, as of this slice, none are known. What remains is a single
//! already-documented, already-accepted symbol collision, not a missing
//! node. Everything this backend does not lower is still refused with a
//! [`CodegenError`], never a panic, and for `Expr` that promise is no
//! longer only this crate's convention to keep: unlike `lex-sys-codegen`,
//! whose `unreachable!`s state an invariant the checker already
//! guarantees, an unsupported node here used to be an ordinary gap in an
//! opt-in, unfinished backend -- and now, for `Expr`, is instead a
//! compile error in this crate itself, caught before any program reaches
//! it.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use lex_sys_codegen::CodegenError;
use lex_sys_ir::{Arm, BinOp, Builtin, Callee, Expr, Func, Place, Program, Slot, Stmt};
use lex_sys_types::{DefId, Type};
use target_lexicon::Triple;

mod body;
mod emit;

use body::*;
use emit::*;

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
