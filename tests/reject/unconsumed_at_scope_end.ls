//~ ERROR still live at the end of this block
//~ RULE linear-value-unconsumed

// §4: linear, not affine. A value that reaches the end of its scope with
// nothing having consumed it is the leak the system exists to prevent.

res struct Ticket { fd: int }

fn open(fd: int) -> [] Ticket {
    return Ticket { fd: fd };
}

fn main() -> [] int {
    if true {
        let f = open(1);
    }
    return 0;
}
