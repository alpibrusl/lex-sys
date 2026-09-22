// `docs/compile-time.md` §4.1 — the decision that costs something.
//
// `unreachable` is a runtime value, so the branch below may never be
// taken and this program might have run for ever without trapping. It is
// refused anyway: an expression with no value is malformed *where it is
// written*, in the way a type error is, and the alternative makes the
// diagnostic depend on how clever a reachability analysis is.
//
// This fixture exists to record that the narrowing was deliberate rather
// than accidental.
//~ ERROR this divides by zero
//~ RULE constant-traps

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    var unreachable = 0;
    if unreachable == 12345 {
        let never = 7 / 0;
        unreachable = never;
    }
    return unreachable;
}
