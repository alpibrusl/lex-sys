//~ ERROR branches disagree about `f`

// §4.2: the join is a real operation. An `if` with no `else` still has two
// arms, and the empty one does not consume `f`.
//
// Rust would insert a dynamic drop flag here. A hidden runtime cost behind a
// static guarantee is exactly what a systems language is judged on, so this
// is a refusal instead.

res struct File { fd: int }

fn close(f: File) -> int {
    let File { fd } = f;
    return fd;
}

fn disagree(f: File, flag: bool) -> int {
    if flag {
        let fd = close(f);
    }
    return 0;
}

fn main() -> int {
    return 0;
}
