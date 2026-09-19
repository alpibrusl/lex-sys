//~ ERROR struct `Point` is declared twice

struct Point { x: int }
struct Point { y: int }

fn main() -> int {
    return 0;
}
