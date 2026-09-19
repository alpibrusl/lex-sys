//~ ERROR still live at the end of this block

// §4: linear, not affine. A value that reaches the end of its scope with
// nothing having consumed it is the leak the system exists to prevent.

res struct File { fd: int }

fn open(fd: int) -> File {
    return File { fd: fd };
}

fn main() -> int {
    if true {
        let f = open(1);
    }
    return 0;
}
