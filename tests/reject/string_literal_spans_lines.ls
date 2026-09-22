//~ ERROR a string literal may not span lines
//~ RULE literal-form

// A literal has one spelling. `docs/strings.md` §4 lists the six
// escapes and says nothing about a quote that never closes, because
// there is nothing to say: the file ended in the middle of a value.

fn main(world: World) -> [] int {
    let s = "never closed;
    return 0;
}
