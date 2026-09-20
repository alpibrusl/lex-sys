//~ ERROR consumed inside this loop

// §4.3: a loop body must leave the live set exactly as it found it, or the
// second iteration uses a value the first one spent. The check is a
// comparison of two sets at the back edge, not a fixpoint.

res struct File { fd: int }

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

fn loop_consume(f: File, n: int) -> [] int {
    var i = 0;
    while i < n {
        let fd = close(f);
        i = i + 1;
    }
    return 0;
}

fn main() -> [] int {
    return 0;
}
