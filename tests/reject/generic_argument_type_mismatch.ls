//~ ERROR expected `int`, found `bool`

// Two arguments at one type parameter, given two different types.
//
// `[T: val]` because `pair_up` drops `b`, which only a copyable type
// allows (`docs/mode-polymorphism.md` §3.1). Without the bound this
// would be refused for *that* instead, and the fixture would stop
// testing what it is named for -- which is how `effect_not_propagated`
// spent a while passing for the wrong reason.
fn pair_up[T: val](a: T, b: T) -> [] T {
    return a;
}

fn main() -> [] int {
    return pair_up(1, true);
}
