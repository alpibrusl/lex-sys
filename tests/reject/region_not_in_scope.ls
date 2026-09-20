//~ ERROR is not a region in scope

// The other half of §5's escape example. `&r File` as a return type names an
// `r` that exists nowhere: a region comes from a `[&r]` parameter or from a
// `borrow` block, and a signature is outside every block.

res struct File { fd: int }

fn escape(f: File) -> [] &r File {
    return f;
}

fn main() -> [] int {
    return 0;
}
