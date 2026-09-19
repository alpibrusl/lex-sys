//~ ERROR `Pair` takes 2 type arguments, but 1 was given

struct Pair[A, B] { first: A, second: B }

fn main() -> int {
    let p: Pair[int] = Pair { first: 1, second: 2 };
    return p.first;
}
