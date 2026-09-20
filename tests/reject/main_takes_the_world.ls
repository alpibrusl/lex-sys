//~ ERROR `main` takes one argument, the `World`

// §8.2: authority enters a program in exactly one place. A `main` that takes
// something else has no `World`, and a program with no `World` can never
// obtain a capability -- so it could not perform an effect even if it tried.

fn main(argc: int) -> [] int {
    return argc;
}
