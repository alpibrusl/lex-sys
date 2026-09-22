//~ ERROR variant `One` is declared twice in `A`
//~ RULE duplicate-declaration

enum A { One, One }

fn main() -> [] int {
    return 0;
}
