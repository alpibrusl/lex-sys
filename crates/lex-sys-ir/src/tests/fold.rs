use super::tests::lower_src;

/// `fold.rs`'s `DEPTH` guard, checked against what it exists to prevent
/// (`docs/fuzzing.md` §4.5): a compile-time recursion deep enough to
/// overflow the compiler's own native stack before the guard gets a
/// chance to refuse. `cargo test` runs each test on its own thread at
/// Rust's default 2 MiB stack, well under a `main` thread's 8 MiB --
/// which is what let a fuzzer-found `fib(1_000_000)` abort the whole
/// test process rather than being cleanly given up on, at the old
/// `DEPTH` of 128. This test reproduces that stack size directly,
/// rather than relying on the fuzzer to land on the right seed, so a
/// regression here fails on its own rather than waiting to be found
/// again by chance.
#[test]
fn deep_compile_time_recursion_does_not_overflow_a_small_stack() {
    let source = "\
fn fib(n: int) -> [] int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(io); release(ffi);
    return fib(1000000) - 2178309;
}
";
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || lower_src(source))
        .expect("the thread spawns");
    let _ = handle.join().expect("compile-time folding should not overflow a 2 MiB stack");
}
