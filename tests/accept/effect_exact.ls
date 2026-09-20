// §7: a row that is exactly what the body performs, all the way up.
//
// Three things this shows and the rejects cannot:
//
//   * a pure helper stays pure. `double` touches nothing, so its row is `[]`,
//     and that is a fact a reader can act on rather than an absence.
//   * a row is transitive. `banner` performs `io` because `emit` does, and
//     `main` because `banner` does.
//   * a row is a *set*. `[io, io]` is `[io]`, and `[io]` written twice in the
//     body costs nothing extra -- union, not count.
//~ STDOUT AB6
//~ EXIT 0

// Nothing here reaches the console, so the row is empty and says so.
fn double(n: int) -> [] int {
    return n * 2;
}

// The grounding: `putchar` is the builtin that performs `io`, and every `io`
// in every row above this one traces back to it.
fn emit(c: int) -> [io] int {
    return putchar(c);
}

// Performs `io` twice; the row is still `[io]`, because a row is a set.
fn banner() -> [io] int {
    emit(65);
    return emit(66);
}

fn main() -> [io] int {
    banner();
    // A pure call inside an effectful function adds nothing to the row.
    let last = emit(48 + double(3));
    emit(10);
    return last - 54;
}
