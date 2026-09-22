//~ ERROR `Nope` is not a struct
//~ RULE not-a-struct

fn main() -> [] int {
    let p = Nope { x: 1 };
    return 0;
}
