//~ ERROR type `Point` is declared twice
//~ RULE duplicate-declaration

struct Point { x: int }
struct Point { y: int }

fn main() -> [] int {
    return 0;
}
