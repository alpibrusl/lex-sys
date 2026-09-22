//~ ERROR already been consumed
//~ RULE linear-use-after-move

// §3: a `res` value may not be copied. Naming `f` twice asks for two of it.

res struct File { fd: int }

struct Pair {
    a: File,
    b: File,
}

fn twice(f: File) -> [] Pair {
    return Pair { a: f, b: f };
}

fn main() -> [] int {
    return 0;
}
