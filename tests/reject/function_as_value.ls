//~ ERROR no function values

fn helper() -> int {
    return 0;
}

fn main() -> int {
    return helper;
}
