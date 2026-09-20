//~ ERROR expected a variant of `A`, found one of `B`

enum A { One }
enum B { Two }

fn main() -> [] int {
    match A::One {
        B::Two => { return 0; }
    }
}
