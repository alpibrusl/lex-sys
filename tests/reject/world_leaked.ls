//~ ERROR `world` is still live here

// The same rule one step earlier. The runtime hands `main` exactly one
// `World` and `main` owns it; ignoring it is ignoring a `res` value.

fn main(world: World) -> [] int {
    return 0;
}
