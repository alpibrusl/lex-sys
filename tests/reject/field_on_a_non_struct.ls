//~ ERROR `int` has no fields
//~ RULE unknown-name

fn main() -> [] int {
    let x = 1;
    return x.y;
}
