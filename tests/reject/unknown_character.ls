// The lexer refuses a character it does not know, where it is written,
// rather than skipping it.
//
// This fixture used `^` until `docs/bitwise.md` gave `^` a meaning, at
// which point it started testing nothing and the suite said so. `@` has
// no meaning and no pending design that wants one -- but the lesson is
// that a fixture whose subject is "this is not a token" has a shelf life,
// and the suite is what notices when it expires.
//~ ERROR unexpected character `@`
//~ RULE unexpected-character

fn main(world: World) -> [] int {
    return 2 @ 3;
}
