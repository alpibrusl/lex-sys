//~ ERROR `main` returns `int`, the process exit status

// The shape of `main` is a contract with the C runtime rather than a matter
// of taste: it takes the `World` and gives back the exit status.

fn main(world: World) -> [] bool {
    let Split { io } = split(world);
    release(io);
    return true;
}
