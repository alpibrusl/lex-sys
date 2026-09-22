//~ ERROR immutable
//~ RULE assign-to-immutable

fn twice(n: int) -> [] int {
    n = n + n;
    return n;
}

fn main() -> [] int {
    return twice(2);
}
