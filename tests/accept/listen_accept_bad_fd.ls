// `docs/llvm-backend.md` §7.20: `listen`/`accept`, the first two of
// `Net`'s four builtins the LLVM backend lowers. Neither takes a
// capability -- the fd's authority was already proved at `bind`, which
// this backend still refuses (`Ffi`/`Net`'s other two builtins,
// `connect` and `bind`, are still outside this slice) -- so both are
// ordinary fixed-signature `libc` calls, checked here the same way
// `Builtin::Sqrt` and every other libc-backed builtin already is.
//
// With no way yet to get a *real* bound fd out of a `--backend llvm`
// program, this fixture checks the other side instead: a deliberately
// invalid fd (`999`, never opened) makes both calls fail the same way
// on any host, `EBADF`, with no real socket and no live connection
// needed. `crates/lex-sys/tests/conformance/backends.rs` runs this
// fixture through both backends and checks they agree;
// `crates/lex-sys-codegen-llvm/src/tests.rs` checks the LLVM path
// directly, self-contained.
//~ EXIT 0

edition 2;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args, net } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io); release(net);

    let l = listen(999, 16);
    let a = accept(999);
    if l < 0 {
        if a < 0 {
            return 0;
        }
    }
    return 1;
}
