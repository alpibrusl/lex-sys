//~ ERROR a reference may not outlive its region

// §5 rule 4: escape is an occurs-check. The block's region is `r`, the
// function's return type can only name `q`, and no `borrow` block outlives a
// region its caller opened -- so the reference has nowhere to go.

res struct File { fd: int }

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

fn escape[&q](f: File, fallback: &q File) -> [] &q File {
    borrow f as &r in {
        return r;
    }
    return fallback;
}

fn main() -> [] int {
    return 0;
}
