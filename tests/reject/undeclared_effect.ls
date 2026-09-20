//~ ERROR performs `io`, which its row [] does not declare

// §7.2: the check at a call site is that the callee's row is a subset of the
// enclosing function's declared row. `putchar` performs `io` and this row is
// empty, so the signature is a lie about what calling it costs.
//
// Holding the capability is not enough on its own: `quiet` *borrows* an
// `Io`, and a borrowed capability is exactly what a row names (§8.2). Owning
// one would discharge the label; borrowing one is what declares it.

fn quiet[&i](io: &!i Io) -> [] int {
    putchar(io, 65);
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io } = split(world);
    release(io);
    return 0;
}
