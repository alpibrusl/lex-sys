//~ ERROR is a built-in type and cannot be redeclared

// A program that could declare its own `Io` could hand itself one. The
// prelude's capability types are as reserved as `int` is, and for a sharper
// reason.

res struct Io {
    fd: int,
}

fn main(world: World) -> [] int {
    let Split { io } = split(world);
    release(io);
    return 0;
}
