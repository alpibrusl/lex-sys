use super::Program;
use super::tests::lower_src;

/// Enough of a program to have authority in it.
const MAIN: &str = " fn main(world: World) -> [] int { \
        let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io); return 0; }";

fn refused(src: &str) -> String {
    lower_src(src).expect_err("this should be refused").message
}

fn accepted(src: &str) -> Program {
    lower_src(src).expect("this should be accepted")
}

#[test]
fn a_capability_has_no_literal_form() {
    // The rule §8.2 rests on: if this were writable, a function given
    // nothing could still print.
    for name in ["Io", "World", "Split"] {
        let src = format!("fn f() -> [] int {{ let c = {name} {{ }}; return 0; }}{MAIN}");
        let message = refused(&src);
        assert!(message.contains("has no literal form"), "{name}: {message}");
    }
}

#[test]
fn a_capability_type_cannot_be_redeclared() {
    let message = refused(&format!("res struct Io {{ fd: int }}{MAIN}"));
    assert!(message.contains("cannot be redeclared"), "{message}");
}

#[test]
fn authority_comes_from_the_world_and_must_be_released() {
    accepted(&format!("fn f() -> [] int {{ return 0; }}{MAIN}"));

    let leaked = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); return 0; }",
    );
    assert!(leaked.contains("still live"), "{leaked}");

    let world = refused("fn main(world: World) -> [] int { return 0; }");
    assert!(world.contains("`world` is still live"), "{world}");
}

#[test]
fn a_released_capability_cannot_be_used_again() {
    let message = refused(
        "fn greet[&i](io: &!i Io) -> [io_write] int { return putchar(io, 65); } \
             fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); \
             release(io); borrow mut io as &!i in { greet(i); } return 0; }",
    );
    assert!(message.contains("nothing left to borrow"), "{message}");
}

#[test]
fn a_capability_is_destroyed_by_release_not_by_destructuring() {
    let message = refused(
        "fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); \
             let Io { } = io; return 0; }",
    );
    assert!(message.contains("destroyed by `release`"), "{message}");
}

#[test]
fn owning_a_capability_discharges_its_label() {
    // §8.2: a row lists what a function *borrows*. `main` prints and its
    // row is still `[]`, because it owns the authority outright -- and
    // that is visible in the parameter list rather than in the row.
    let program = accepted(
        "fn greet[&i](io: &!i Io) -> [io_write] int { return putchar(io, 65); } \
             fn main(world: World) -> [] int { let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); \
             borrow mut io as &!i in { greet(i); } release(io); return 0; }",
    );
    let main = program.func(program.find("main").expect("main"));
    assert!(main.effects.is_pure(), "{}", main.effects);
}

#[test]
fn borrowing_a_capability_does_not_discharge_it() {
    // The other half of the same rule, and the one that keeps every row
    // in the program from being empty.
    let message = refused("fn quiet[&i](io: &!i Io) -> [] int { putchar(io, 65); return 0; }");
    assert!(message.contains("performs `io_write`"), "{message}");
}

#[test]
fn a_capability_costs_nothing_at_runtime() {
    // §8.1: capabilities erase except where they carry data, and these
    // carry none. `World` and `Io` are zero-sized, so threading one adds
    // no machine parameter at all.
    let program = accepted(&format!("fn f() -> [] int {{ return 0; }}{MAIN}"));
    let main = program.func(program.find("main").expect("main"));
    assert_eq!(main.n_params, 1, "`main` takes the World");
    assert!(
        super::leaf_free(&main.slots[0]),
        "a `World` should occupy no machine value, got {:?}",
        main.slots[0]
    );
}
