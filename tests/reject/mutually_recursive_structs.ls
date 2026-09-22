//~ ERROR contains itself
//~ RULE infinite-type

struct A { b: B }
struct B { a: A }

fn main() -> [] int {
    return 0;
}
