//~ ERROR immutable
//~ RULE assign-to-immutable

fn main() -> [] int {
    let x = 1;
    x = 2;
    return x;
}
