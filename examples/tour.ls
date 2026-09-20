// tour.ls — everything the language currently has, in one readable file.
//
// `hello.ls` is deliberately minimal and `rational.ls` is a real 250-line
// program; this sits between them. Each section is the smallest honest
// demonstration of one feature, in the order the milestones added them.
//
//~ STDOUT M0: 7 5 3 1
//~ STDOUT M1 bool: 1010010
//~ STDOUT M1 struct: (3, 4) -> 25
//~ STDOUT M1 enum: 0 12 20
//~ STDOUT M1 generic: 5 3 z
//~ STDOUT M2 linear: 4 7 9 5 6
//~ STDOUT M2 borrow: 4 8 12
//~ STDOUT M2 unique: 3 5 5
//~ STDOUT M2 effects: 42
//~ STDOUT M2 capability: 88
//~ STDOUT M2 foreign: 7 9
//~ STDOUT M2 arena: 1 4 9 -> 14
//~ STDOUT M3 arithmetic: 6 1
//~ STDOUT M3 slice: 3 1 4 1 5 -> 14
//~ STDOUT M3 string: hello (5 bytes, e is 101)
//~ STDOUT M3 file: on disk (7)
//~ STDOUT M3 heap: 3 1 4 -> 8 (freed)
//~ STDOUT M3 reading: 8 3 8 (kept)
//~ EXIT 0

// ---------------------------------------------------------------- output ---
// There are still no strings, so text is written a byte at a time. M3's
// slices are what change this.
//
// Every signature below declares an effect row between `->` and the return
// type. `[]` is how a function says it is pure; `[io]` says it reaches the
// console. The row is *exact* -- declaring an effect you do not perform is
// as much an error as performing one you did not declare -- and it is
// transitive.
//
// And every one of them takes an `io: &!i Io`. That is not boilerplate: it
// is the reason the row can be trusted. A function is handed the authority
// to print or it cannot print, and `main` at the bottom of this file is the
// only place any of it comes from.

fn newline[&i](io: &!i Io) -> [io] int {
    return putchar(io, 10);
}

fn space[&i](io: &!i Io) -> [io] int {
    return putchar(io, 32);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// ------------------------------------------------------------- M0: ints ----
// Functions, arithmetic, `if`/`else`, `while`, and `let`/`var` bindings.
// `let` is immutable; parameters are too.

fn label_m0[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 48); putchar(io, 58);      // "M0:"
    return 0;
}

fn m0[&i](io: &!i Io) -> [io] int {
    label_m0(io);
    space(io); print_nat(io, 1 + 2 * 3);              // precedence: 7
    space(io); print_nat(io, 10 - 3 - 2);             // left-associative: 5
    space(io); print_nat(io, 0 - (-6 / 2 + 6) + 6);   // truncating division: 3
    space(io); print_nat(io, 7 % 3);                  // remainder: 1
    return newline(io);
}

// ------------------------------------------------------------ M1: bool -----
// A comparison has a type. `if` and `while` require it, and there is no
// conversion in either direction. `&&` and `||` short-circuit.

fn digit[&i](io: &!i Io, b: bool) -> [io] int {
    if b {
        return putchar(io, 49);
    }
    return putchar(io, 48);
}

fn m1_bool[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 49); space(io);            // "M1 "   // "M1:"
    putchar(io, 98); putchar(io, 111); putchar(io, 111); putchar(io, 108); putchar(io, 58); space(io);
    digit(io, 2 < 3);
    digit(io, 3 <= 2);
    digit(io, 4 == 4);
    digit(io, 5 != 5);
    digit(io, true && false);
    digit(io, true || false);
    digit(io, !true);
    return newline(io);
}

// ---------------------------------------------------------- M1: structs ----
// Values with named fields, passed and returned by value, nested freely.

struct Vec2 {
    x: int,
    y: int,
}

fn length_squared(v: Vec2) -> [] int {
    return v.x * v.x + v.y * v.y;
}

fn m1_struct[&i](io: &!i Io) -> [io] int {
    let v = Vec2 { x: 3, y: 4 };
    putchar(io, 77); putchar(io, 49); space(io);            // "M1 "
    putchar(io, 115); putchar(io, 116); putchar(io, 114); putchar(io, 117); putchar(io, 99);
    putchar(io, 116); putchar(io, 58); space(io);
    putchar(io, 40); print_nat(io, v.x); putchar(io, 44); space(io); print_nat(io, v.y); putchar(io, 41);
    space(io); putchar(io, 45); putchar(io, 62); space(io);
    print_nat(io, length_squared(v));               // 25
    return newline(io);
}

// ------------------------------------------------------------ M1: enums ----
// A value that is exactly one of several shapes. `match` must cover every
// variant, and the compiler names the ones you forgot.

enum Shape {
    Empty,
    Circle(int),
    Rect(int, int),
}

fn area(s: Shape) -> [] int {
    match s {
        Shape::Empty => { return 0; }
        Shape::Circle(r) => { return 3 * r * r; }
        Shape::Rect(w, h) => { return w * h; }
    }
}

fn m1_enum[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 49); space(io);            // "M1 "
    putchar(io, 101); putchar(io, 110); putchar(io, 117); putchar(io, 109); putchar(io, 58); space(io);
    print_nat(io, area(Shape::Empty));              // 0
    space(io); print_nat(io, area(Shape::Circle(2))); // 12
    space(io); print_nat(io, area(Shape::Rect(4, 5)));// 20
    return newline(io);
}

// --------------------------------------------------------- M1: generics ----
// One definition, monomorphised per type it is used at. `Opt[T]` is the
// obvious first use; `T` is settled by the arguments at each call.

enum Opt[T] {
    None,
    Some(T),
}

fn unwrap_or[T](o: Opt[T], fallback: T) -> [] T {
    match o {
        Opt::None => { return fallback; }
        Opt::Some(value) => { return value; }
    }
}

fn m1_generic[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 49); space(io);            // "M1 "
    putchar(io, 103); putchar(io, 101); putchar(io, 110); putchar(io, 101); putchar(io, 114);
    putchar(io, 105); putchar(io, 99); putchar(io, 58); space(io);

    // Instantiated at `int` twice...
    print_nat(io, unwrap_or(Opt::Some(5), 0));      // 5
    let missing: Opt[int] = Opt::None;          // the annotation settles `T`
    space(io); print_nat(io, unwrap_or(missing, 3));  // 3

    // ...and at `bool`, which is a second copy of the same source.
    space(io);
    if unwrap_or(Opt::Some(true), false) {
        putchar(io, 122);                           // 'z'
    } else {
        putchar(io, 45);
    }
    return newline(io);
}

// ------------------------------------------------- M2: linear resources ----
// Every type has a mode. `val` is unrestricted -- copyable, discardable, no
// obligations -- and everything above this line is `val`. `res` is linear:
// exactly one use, no implicit copy, and **no implicit discard**.
//
// Linear, not affine. A `res` value that reaches the end of its scope
// unconsumed is a compile error, because the case affine drops silently is
// the one this system exists to prevent. And there is no destructor: a
// destructor is code that runs at a point nobody wrote, which would make the
// effect row on the enclosing function a lie.
//
// So a resource is destroyed by naming the function that knows how, and that
// function ends in taking the value apart.

res struct Ticket {
    serial: int,
}

fn issue(serial: int) -> [] Ticket {
    return Ticket { serial: serial };
}

// Takes ownership and hands it back, so the caller still owes one
// consumption -- ownership moved twice, not shared once.
fn stamp(t: Ticket) -> [] Ticket {
    let Ticket { serial } = t;
    return Ticket { serial: serial + 1 };
}

// The terminal consumer. Destructuring spends the whole and produces the
// parts; these parts are `int`, which is `val`, so nothing is owed after.
fn redeem(t: Ticket) -> [] int {
    let Ticket { serial } = t;
    return serial;
}

// `res` by inference: mode is structural, so an aggregate holding a `res`
// member is `res` without anyone writing the word.
struct Booking {
    outbound: Ticket,
    inbound: Ticket,
}

fn redeem_both(b: Booking) -> [] int {
    let Booking { outbound, inbound } = b;
    return redeem(outbound) + redeem(inbound);
}

// Both paths consume, so they agree about what is live at the merge point.
// An `if` whose `else` did not consume is refused rather than fixed up with a
// runtime drop flag -- see `tests/reject/branches_disagree.ls`.
fn redeem_either(t: Ticket, as_is: bool) -> [] int {
    if as_is {
        return redeem(t);
    }
    return redeem(stamp(t));
}

fn m2_linear[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 108); putchar(io, 105); putchar(io, 110); putchar(io, 101); putchar(io, 97);
    putchar(io, 114); putchar(io, 58); space(io);         // "linear: "

    print_nat(io, redeem(issue(4)));                                    // 4
    space(io); print_nat(io, redeem(stamp(issue(6))));                    // 7
    space(io); print_nat(io, redeem_both(Booking {
        outbound: issue(2),
        inbound: issue(7),
    }));                                                            // 9
    space(io); print_nat(io, redeem_either(issue(5), true));              // 5
    space(io); print_nat(io, redeem_either(issue(5), false));             // 6
    return newline(io);
}

// ------------------------------------------------------ M2: borrowing ----
// Linearity alone says a resource is used exactly once. That leaves no way to
// *look* at one without spending it -- `t.serial` on an owned `Ticket` is
// refused, because reading a part out of a value without taking it apart is a
// non-owning read, and a non-owning read is a borrow.
//
// So: `borrow x as &r in { .. }`. The block introduces a region `r`, freezes
// `x` for its duration, and binds a reference `r` of type `&r Ticket`. The
// name does both jobs, which is how §5 writes it.
//
// There is no borrow checker. A reference's validity is a *lexical* fact: a
// region is a block, full stop. No non-lexical lifetimes, no inference, no
// variance -- a binding is `Owned` or `Frozen`, set at block entry and
// restored at block exit, and escape is an occurs-check on one type.

// Region-polymorphic, with the region written (§5.1). At a call site the
// parameter is instantiated with the caller's region: one name, one
// assignment, nothing that can fail to terminate.
fn serial_of[&r](t: &r Ticket) -> [] int {
    return t.serial;
}

// `src <= dst` says `dst` outlives `src` (§5.2). Checking it is a walk up the
// stack of enclosing blocks -- O(depth), no fixpoint, total.
fn later_of[&dst, &src where src <= dst](a: &dst Ticket, b: &src Ticket) -> [] int {
    return serial_of(a) + serial_of(b);
}

fn m2_borrow[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 98); putchar(io, 111); putchar(io, 114); putchar(io, 114); putchar(io, 111);
    putchar(io, 119); putchar(io, 58); space(io);         // "borrow: "

    let held = issue(4);

    borrow held as &r in {
        // Reading through the reference, which the owned value refuses.
        print_nat(io, r.serial);

        // Shared borrows nest: freezing is not exclusive, because two
        // readers neither move the value nor change it.
        space(io);
        borrow held as &inner in {
            print_nat(io, serial_of(r) + serial_of(inner));
        }

        // `r` comes from the enclosing block, so it outlives `inner` and may
        // be used where `&inner` is expected. Nothing else coerces.
        space(io);
        borrow held as &inner in {
            print_nat(io, later_of(r, inner) + serial_of(r));
        }
    }

    // Owned again, and still owed exactly one consumption.
    let spent = redeem(held);
    return newline(io);
}

// ------------------------------------------------ M2: unique borrows ----
// A shared borrow promises the value will not change, which is why several
// may nest and why nothing has to be written back when the block closes.
// `borrow mut x as &!r in { .. }` makes the opposite promise: it *locks* `x`
// for the block, and the reference may be written through.
//
// Locked is stronger than frozen. Nothing else may touch `x` at all -- not a
// read, not a second borrow, not a move -- and that is what makes `&!r` mean
// unique. If the owner could still read the value, the reference would not
// be the only way to reach it.
//
// Writing through it needs somewhere to write *to*: a place is a whole
// binding, or a field reached through a unique reference. A field of an
// owned local is deliberately not one -- that is a partial write, and what a
// partial write means for a binding holding a `res` field is a question §4
// does not answer.

struct Meter {
    reading: int,
    step: int,
}

fn advance[&r](m: &!r Meter) -> [] int {
    m.reading = m.reading + m.step;
    return m.reading;
}

fn m2_unique[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 117); putchar(io, 110); putchar(io, 105); putchar(io, 113); putchar(io, 117);
    putchar(io, 101); putchar(io, 58); space(io);         // "unique: "

    var meter = Meter { reading: 1, step: 2 };

    borrow mut meter as &!r in {
        print_nat(io, advance(r));
        space(io); print_nat(io, advance(r));
    }

    // Owned again, and carrying what the reference wrote. The value lived in
    // a buffer for the block and was read back when it closed, which is
    // sound precisely because the lock meant nothing else could have moved on.
    space(io); print_nat(io, meter.reading);
    return newline(io);
}

// -------------------------------------------------------- M2: effects ----
// An effect row is a canonically ordered *set* of labels -- no duplicates,
// no row variables. Two operations are needed and only two: union, to work
// out what a body performs, and subset, to check that against what it
// declared. Both are linear in a small, statically bounded number of labels.
//
// Rows are declared at boundaries and inferred only inside a body, where
// there is nothing to infer but a fold over the calls. Whole-program effect
// inference is exactly the non-local analysis the totality commitment
// forbids, and an inferred row is a contract nobody wrote that everybody
// depends on.
//
// There is no registry of legal labels and none is needed. Every `io` here
// traces back to `putchar`, the one builtin that performs it; a label with
// nothing underneath it can never appear in an exact row, so it is refused
// the moment it is written.

// Pure, and says so. A reader can act on that -- and so can the checker,
// which is what makes `examples {}` blocks runnable at check time in Lex.
fn triple(n: int) -> [] int {
    return n * 3;
}

// Performs `io`, because `print_nat` does. Nothing else about the body
// matters to the row.
fn show[&i](io: &!i Io, n: int) -> [io] int {
    return print_nat(io, n);
}

fn m2_effects[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 101); putchar(io, 102); putchar(io, 102); putchar(io, 101); putchar(io, 99);
    putchar(io, 116); putchar(io, 115); putchar(io, 58); space(io);   // "effects: "

    // A pure call inside an effectful body adds nothing to the row.
    show(io, triple(14));
    return newline(io);
}

// ---------------------------------------------------------- M2: arenas ----
// The other thing that opens a region, and the answer to the question §5
// leaves hanging: if a reference's validity is a block, where does data that
// outlives its *creator* live?
//
// `region a { .. }` opens an arena. `alloc[a](v)` puts a value in it and
// hands back `&!a T` -- an ordinary unique reference, carrying the region,
// subject to every rule §5 already gave. At block exit the whole arena goes
// in one call: no traversal, no per-object bookkeeping, no finalisers.
//
// The escape rule is not a new rule. Nothing whose type mentions `a` leaves
// the block, checked by the same occurs-check that stops a `borrow`'s
// reference escaping -- which is the claim §6 is making. An arena's lifetime
// and a borrow's lifetime are one mechanism, and nesting is §5.2's stack
// over again: an inner arena may hold what an outer one allocated, never the
// reverse.
//
// And arenas hold `val` data only (§6.1). Releasing one reclaims memory; it
// does not close files or release capabilities. A `res` value inside would
// have its memory taken back with its obligation undischarged -- a leak with
// a static blessing, which is the exact case §4 refuses to let affine typing
// paper over.

struct Cell {
    value: int,
}

// Written against a *shared* reference, and called below on something the
// arena handed back as unique: `&!r T` is `&r T` plus permission to write.
fn cell_value[&r](c: &r Cell) -> [] int {
    return c.value;
}

fn m2_arena[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 97); putchar(io, 114); putchar(io, 101); putchar(io, 110);
    putchar(io, 97); putchar(io, 58); space(io);          // "arena: "

    var total = 0;
    region a {
        var n = 1;
        while n <= 3 {
            let cell = alloc[a](Cell { value: 0 });
            // Written through the unique reference, read through a function
            // that only asked to borrow.
            cell.value = n * n;
            if n > 1 {
                space(io);
            }
            print_nat(io, cell_value(cell));
            total = total + cell_value(cell);
            n = n + 1;
        }
    }
    // Three allocations, one release, and it already happened.

    space(io); putchar(io, 45); putchar(io, 62); space(io);   // " -> "
    print_nat(io, total);
    return newline(io);
}

fn run[&i](io: &!i Io) -> [io] int {
    m0(io);
    m1_bool(io);
    m1_struct(io);
    m1_enum(io);
    m1_generic(io);
    m2_linear(io);
    m2_borrow(io);
    m2_unique(io);
    m2_effects(io);
    return 0;
}

// --------------------------------------------------- M2: capabilities ----
// The thesis, and the last piece of it. An effect row says what a function
// does; a capability is what lets it. `[io]` on a signature means the
// function was handed an `&!i Io` it did not create, and after that reading
// the row and reading the parameter list are the same act.
//
// A capability is an ordinary `res` value -- no special kind, no special
// syntax -- so everything in this file already applies to it. It is linear,
// so it is released exactly once. It is borrowed rather than moved, so a
// callee cannot consume its caller's authority. And it has no literal form,
// so the only `Io` that exists is the one inside the `World` the runtime
// handed `main`.
//
// That is the whole safety story, and it is stated as a type: a function
// that is not given a capability cannot perform its effect.

fn twice[&i](io: &!i Io, n: int) -> [io] int {
    print_nat(io, n);
    return print_nat(io, n);
}

fn m2_capability[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 99); putchar(io, 97); putchar(io, 112); putchar(io, 97);
    putchar(io, 98); putchar(io, 105); putchar(io, 108); putchar(io, 105);
    putchar(io, 116); putchar(io, 121); putchar(io, 58); space(io);

    // Prints "8" twice -- and could not print at all without the `io` it was
    // handed. Delete the parameter and the body stops compiling.
    twice(io, 8);
    return newline(io);
}

// ------------------------------------------------------- M2: foreign ----
// C's effects stop being invisible at the exact point they enter the
// program. A foreign call is an effect like any other, and the capability
// that authorises it names the library: `Ffi("libc")` reaches libc and
// nothing else.
//
// Where does one come from? `split` hands out an `Ffi` that names *no*
// library -- authority over nothing. `narrow` is the only way to get one
// that names something, and narrowing goes one way only: `Ffi("")` can
// become `Ffi("libc")`, and `Ffi("libc")` can never become anything wider.
// That is the same commitment `lex-os` makes for manifests, and for the
// same reason -- a program must not be able to grant itself what it was not
// given.
//
// The `extern` declaration is the only place a foreign signature is
// written. It says which library, and the row says it again, and the two
// have to agree: a declaration that took no capability, or took one for a
// different library, is refused where it is written rather than where it is
// called.

// libc's `long labs(long)`. The capability parameter is the door, and there
// is no other.
extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

// Borrows both capabilities, so it declares both labels. A caller reading
// this signature knows this function reaches one named library and the
// console, and that it can do nothing else -- there is nothing else it was
// handed.
fn m2_foreign[&f, &i](libc: &f Ffi("libc"), io: &!i Io) -> [ffi("libc"), io] int {
    putchar(io, 77); putchar(io, 50); space(io);          // "M2 "
    putchar(io, 102); putchar(io, 111); putchar(io, 114); putchar(io, 101);
    putchar(io, 105); putchar(io, 103); putchar(io, 110); putchar(io, 58); space(io);

    // The capability is checked and then erased: what libc receives is the
    // integer and nothing else, because a capability carries no data.
    print_nat(io, labs(libc, 0 - 7));                     // 7
    space(io); print_nat(io, labs(libc, 9));              // 9
    return newline(io);
}

// ----------------------------------------------------- M3: arithmetic ----
// `int` is 64-bit two's complement, and `+`, `-`, `*` and unary `-` produce
// the right answer or they **trap**. They never wrap.
//
// Wrapping silently is *defined* behaviour -- C has it for unsigned types,
// Rust has it in release builds -- so it is not undefined behaviour being
// refused here. It is a silently wrong answer, and the reason to refuse one
// is that the wrong answer propagates and the stopped process does not.
//
// Wraparound is still the intent in a hash, a checksum or a cycle counter,
// and a language that cannot say it forces a workaround worse than the thing
// it forbids. So it is spelled out. The asymmetry is the whole design: `+`
// is what you write when you mean arithmetic, `wrapping_add` is what you
// write when you mean the bits, and you cannot get the second by accident.
//
// See `docs/defined-behaviour.md`, where every rule has a fixture.

fn highest() -> [] int {
    return 9223372036854775807;
}

fn m3_arithmetic[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 51); space(io);          // "M3 "
    putchar(io, 97); putchar(io, 114); putchar(io, 105); putchar(io, 116);
    putchar(io, 104); putchar(io, 109); putchar(io, 101); putchar(io, 116);
    putchar(io, 105); putchar(io, 99); putchar(io, 58); space(io);

    // Checked, and right. `highest() + 1` here would kill the process.
    print_nat(io, 2 + 4);

    // Asked for, and wrapped: one past the top is the bottom, which is
    // negative -- so the digit is 1.
    space(io); digit(io, wrapping_add(highest(), 1) < 0);
    return newline(io);
}

// --------------------------------------------------------- M3: slices ----
// The first half of "enough to write real programs", and the thing strings
// will be made of.
//
// A slice is an *ordinary reference*. `[T]` is a referent -- a run of `T`s
// whose length is a runtime value -- and `&!a [T]` points at one. That is
// the whole design: every rule §5 gave references applies to a slice with
// no second mechanism. It carries a region, it cannot escape that region,
// a unique one coerces to a shared one, and it is `val` because every
// reference is.
//
// The length travels *in* the slice, beside the pointer, which is why
// `len` reads a value rather than computing one -- and why `s[i]` can be
// checked against it. Every index is bounds-checked and an out-of-range one
// traps; the comparison is unsigned, so a negative index is caught by the
// same instruction. There is no unchecked form, because reading past the
// end of an allocation is the undefined behaviour this language does not
// have (`docs/defined-behaviour.md` §4).

fn sum_of[&r](xs: &r [int]) -> [] int {
    var total = 0;
    var i = 0;
    while i < len(xs) {
        total = total + xs[i];
        i = i + 1;
    }
    return total;
}

fn m3_slice[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 51); space(io);          // "M3 "
    putchar(io, 115); putchar(io, 108); putchar(io, 105); putchar(io, 99);
    putchar(io, 101); putchar(io, 58); space(io);         // "slice: "

    var total = 0;
    region a {
        // Five zeroes, in the arena. The length is a runtime value, which
        // is what makes this a slice rather than an array -- an array's
        // length would live in its type.
        let digits = alloc_slice[a](5, 0);
        digits[0] = 3; digits[1] = 1; digits[2] = 4; digits[3] = 1; digits[4] = 5;

        var n = 0;
        while n < len(digits) {
            if n > 0 {
                space(io);
            }
            print_nat(io, digits[n]);
            n = n + 1;
        }
        // Written through a unique slice above, read through a shared one
        // here: `sum_of` only asked to borrow.
        total = sum_of(digits);
    }
    // The slice and its arena are both gone by here, in one call.

    space(io); putchar(io, 45); putchar(io, 62); space(io);   // " -> "
    print_nat(io, total);
    return newline(io);
}

// -------------------------------------------------------- M3: strings ----
// The last thing M3 needed, and the one that took a design document before
// it took code (`docs/strings.md`).
//
// A string is a run of bytes and claims **no encoding**. A validated type
// would have to answer what an *invalid* one is, and every answer costs a
// fallible constructor everywhere or a lie somewhere; bytes answer it by
// not asking. Decoding is library work, over exactly this slice type.
//
// So `str` is not a type. A string is `&r [byte]` -- an ordinary slice,
// therefore an ordinary reference -- and regions, the escape check, the
// unique-to-shared coercion and `val` mode all came for free, the same way
// they did for slices. A literal lives in `static`, which outlives
// everything, and is *shared*: two occurrences of `"ok"` may be the same
// bytes, so nothing may write through one.
//
// `byte` is **storage, not arithmetic**: no `+`, and `byte_of` / `int_of`
// convert. That is what keeps `defined-behaviour.md` §8's deferral of
// unsigned widths intact -- a type you cannot add to never asks whether
// adding traps. `byte_of` traps outside 0..255 rather than truncating,
// because truncation is the silently wrong answer §2.1 already refused.
// `==` is allowed, because comparing storage is not arithmetic.

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn m3_string[&i](io: &!i Io) -> [io] int {
    putchar(io, 77); putchar(io, 51); space(io);          // "M3 "
    putchar(io, 115); putchar(io, 116); putchar(io, 114); putchar(io, 105);
    putchar(io, 110); putchar(io, 103); putchar(io, 58); space(io);

    let word = "hello";
    write_all(io, word);

    // `len` is the *byte* length: this design claims no encoding, so there
    // are no characters to count.
    space(io); putchar(io, 40);                           // " ("
    print_nat(io, len(word));
    space(io); write_all(io, "bytes,");

    // Indexing yields a `byte`, which has to be widened to be printed --
    // the conversion is where the range is checked, visibly.
    space(io); write_all(io, "e is");
    space(io); print_nat(io, int_of(word[1]));
    putchar(io, 41);                                      // ")"
    return newline(io);
}

// ----------------------------------------------------- M3: the filesystem ----
// M3's last mile, and an *application* of §8 rather than a new idea
// (`docs/filesystem.md`).
//
// `Fs(prefix)` is a capability carrying a path, narrowed by the same prefix
// extension `Ffi` uses -- except that a path prefix extends at a `/`, so
// `/tmp` contains `/tmp/a` and pointedly does not contain `/tmpevil`. The
// effect labels carry the prefix too, which is why the row below says
// `fs_read("/tmp")` and not `fs_read`: a caller learns which part of the
// filesystem this touches without opening the body.
//
// The operations are **builtins, not `extern fn`** (§2). An `extern` would
// be gated by `Ffi("libc")`, and then holding the FFI capability would open
// any path while `Fs` contributed nothing. The authority that guards the
// filesystem has to be the one that names the filesystem.
//
// Two checks are runtime rules, because the prefix is in the type and the
// path is a value: a path outside the granted prefix traps, and a path
// containing `..` traps rather than being normalised (§4.1). A missing file
// is `-1` -- an outcome, not a broken promise.
//
// What `Fs` does *not* do is sandbox: a program holding `Ffi("libc")` can
// declare `extern fn open`. The claim is that authority is visible in the
// types, and that `main` chooses. `examples/lines.ls` is the whole tool.

fn m3_file[&f, &i](
    fs: &f Fs("/tmp"),
    io: &!i Io,
) -> [fs_read("/tmp"), fs_write("/tmp"), io] int {
    write_all(io, "M3 file:"); space(io);

    let wrote = fs_write(fs, "/tmp/lex-sys-tour.txt", "on disk");

    var status = 0;
    region a {
        // The buffer is longer than the file: the *read* decides how many
        // bytes there are, not the buffer.
        let buffer = alloc_slice[a](32, byte_of(0));
        let read = fs_read(fs, "/tmp/lex-sys-tour.txt", buffer);
        var n = 0;
        while n < read {
            putchar(io, int_of(buffer[n]));
            n = n + 1;
        }
        space(io); putchar(io, 40);                       // " ("
        print_nat(io, wrote);
        putchar(io, 41);                                  // ")"
        status = read;
    }
    newline(io);
    return status - 7;
}

// ---------------------------------------------------------- M3: the heap ----
// The last thing M3 added, and the one that closes M2's list
// (`docs/heap.md`).
//
// Everything above this lives in a frame or an arena, and both are
// *lexical*: a lifetime is a block. The heap is for the values whose
// lifetime the *program* decides -- something that outlives the call that
// made it, or a type defined in terms of itself.
//
// `Heap` is the fifth capability. It carries nothing, like `Io` and unlike
// `Ffi(library)` and `Fs(prefix)`, because a heap has no parts to name and
// so nothing to narrow. It is borrowed *uniquely*, because an allocator has
// state and those other two are only keys. Allocation is an effect and
// `[heap]` is how a function says it allocates.
//
// `Box[T]` is one value and one allocation, and it is `res` whatever `T` is
// -- so §4's rule applies to it unchanged, and **the general heap cannot
// leak**: a box that is never unboxed is a compile error. Nor can it
// double-free, because `unbox` consumes. That is a stronger guarantee than
// either escape hatch §9 will offer, and it costs nothing at run time: a
// box is a pointer, with no header, no refcount and no tag.
//
// The payoff is a type that contains itself. `List` below is refused
// without the `Box` -- it has no finite size -- and compiles with it,
// because a box is one pointer however large what it points at is. Note
// what `drain` does: `match` takes ownership, so reading this list is the
// same act as freeing it, and a version of it that forgot a node would not
// compile. See `examples/tree.ls` for the same idea with two children.

enum List {
    Empty,
    Cons(int, Box[List]),
}

fn m3_heap[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    write_all(io, "M3 heap:"); space(io);

    // Built backwards, so this reads 3 1 4.
    var list = List::Empty;
    list = push(heap, list, 4);
    list = push(heap, list, 1);
    list = push(heap, list, 3);

    // `drain` leaves a trailing space after the last value, so this does
    // not add one.
    let total = drain(heap, io, list);
    write_all(io, "->"); space(io);
    print_nat(io, total);
    space(io); write_all(io, "(freed)");
    return newline(io);
}

fn push[&h](heap: &!h Heap, rest: List, value: int) -> [heap] List {
    return List::Cons(value, box(heap, rest));
}

// One pass that prints and frees. `rest` is the box the match handed over;
// `unbox` ends that node and yields the tail.
fn drain[&h, &i](heap: &!h Heap, io: &!i Io, list: List) -> [heap, io] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => {
            print_nat(io, value);
            space(io);
            let tail = unbox(heap, rest);
            return value + drain(heap, io, tail);
        }
    }
}

// ------------------------------------------- M3: reading a reference ----
// The gap the heap section left, and the one that closes it
// (`docs/reading-references.md`).
//
// Two things were impossible above. A scalar behind a reference could not
// be read -- `&r int` has existed since M2 with nothing to do to one. And
// a recursive structure could not be read *without destroying it*, because
// `match` required ownership: the walk that printed the list above is the
// walk that freed it, and a structure you can only read by consuming is a
// structure you can read once.
//
// They are one gap. The rule that closes it is one sentence:
//
//     A reference gives references.
//
// Matching `&l List` binds every payload as a reference into the list,
// carrying the scrutinee's mode and its region. Nothing moves out of a
// reference, ever -- which is what keeps linearity intact, because a `res`
// payload binds as a *borrow* of that `res` and creates no obligation. And
// `*r` reads what a reference points at, for a `val` referent; copying a
// `res` would duplicate an obligation, so it is refused.
//
// No binding modes and nothing inferred (§2.1): match an owned enum and
// you get owned payloads, match a reference and you get references. Which
// one you are in is visible one line up, in the scrutinee.

fn sum_kept[&l](list: &l List) -> [] int {
    match list {
        List::Empty => { return 0; }
        // `value` is `&l int` and `rest` is `&l Box[List]`. `contents`
        // follows the box to `&l List`, for exactly as long as `l` lasts.
        List::Cons(value, rest) => { return *value + sum_kept(contents(rest)); }
    }
}

fn count_kept[&l](list: &l List) -> [] int {
    match list {
        List::Empty => { return 0; }
        // `_` through a reference discards nothing: this match never owned
        // the value, so there is nothing to drop.
        List::Cons(_, rest) => { return 1 + count_kept(contents(rest)); }
    }
}

fn m3_reading[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io] int {
    write_all(io, "M3 reading:"); space(io);

    var list = List::Empty;
    list = push(heap, list, 4);
    list = push(heap, list, 1);
    list = push(heap, list, 3);

    // Read three times. None of these spends the list: `sum_kept` and
    // `count_kept` hold no capability at all, so their rows are `[]` and
    // they could not free anything if they tried.
    borrow list as &l in {
        print_nat(io, sum_kept(l));
        space(io);
        print_nat(io, count_kept(l));
        space(io);
        print_nat(io, sum_kept(l));
    }
    space(io); write_all(io, "(kept)");
    newline(io);

    // Still owned here, and still owing exactly one traversal that ends it.
    return drain_quiet(heap, list);
}

fn drain_quiet[&h](heap: &!h Heap, list: List) -> [heap] int {
    match list {
        List::Empty => { return 0; }
        List::Cons(value, rest) => {
            let tail = unbox(heap, rest);
            return value + drain_quiet(heap, tail);
        }
    }
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io, ffi, fs, heap } = split(world);
    // §7.4: attenuation, and the two narrowings in this program. From here
    // the foreign authority in this file reaches libc and no other library,
    // and its filesystem authority reaches `/tmp` and nowhere else. Neither
    // can be widened again, by this function or any it calls.
    let libc = narrow(ffi, "libc");
    let tmp = narrow(fs, "/tmp");

    var status = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow libc as &f in {
        borrow tmp as &t in {
        borrow mut heap as &!p in {
        borrow mut io as &!i in {
            status = run(i);
            m2_capability(i);
            m2_foreign(f, i);
            m2_arena(i);
            m3_arithmetic(i);
            m3_slice(i);
            m3_string(i);
            m3_file(t, i);
            m3_heap(p, i);
            m3_reading(p, i);
        }
        }
        }
    }

    // Both are resources, so both are destroyed exactly once. A program that
    // forgets either does not compile.
    release(libc);
    release(tmp);
    release(heap);
    release(io);
    return status;
}
