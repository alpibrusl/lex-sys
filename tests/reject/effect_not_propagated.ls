//~ ERROR performs `io`, which its row [] does not declare

// A row is transitive: `caller` performs whatever `shout` performs, because
// calling it is how the effect happens. This is what makes a row at the top
// of a program mean anything -- `main`'s row is the whole program's.

fn shout() -> [io] int {
    return putchar(33);
}

fn caller() -> [] int {
    return shout();
}

fn main() -> [] int {
    return 0;
}
