//~ ERROR cannot be discarded

// §4.1: there is no `drop`. A statement that evaluates a `res` value and
// throws the result away has named no consumer.

res struct File { fd: int }

fn open(fd: int) -> [] File {
    return File { fd: fd };
}

fn main() -> [] int {
    open(1);
    return 0;
}
