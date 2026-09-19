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
//~ EXIT 0

// ---------------------------------------------------------------- output ---
// There are still no strings, so text is written a byte at a time. M3's
// slices are what change this.

fn newline() -> int {
    return putchar(10);
}

fn space() -> int {
    return putchar(32);
}

fn print_nat(n: int) -> int {
    if n >= 10 {
        print_nat(n / 10);
    }
    return putchar(48 + n % 10);
}

// ------------------------------------------------------------- M0: ints ----
// Functions, arithmetic, `if`/`else`, `while`, and `let`/`var` bindings.
// `let` is immutable; parameters are too.

fn label_m0() -> int {
    putchar(77); putchar(48); putchar(58);      // "M0:"
    return 0;
}

fn m0() -> int {
    label_m0();
    space(); print_nat(1 + 2 * 3);              // precedence: 7
    space(); print_nat(10 - 3 - 2);             // left-associative: 5
    space(); print_nat(0 - (-6 / 2 + 6) + 6);   // truncating division: 3
    space(); print_nat(7 % 3);                  // remainder: 1
    return newline();
}

// ------------------------------------------------------------ M1: bool -----
// A comparison has a type. `if` and `while` require it, and there is no
// conversion in either direction. `&&` and `||` short-circuit.

fn digit(b: bool) -> int {
    if b {
        return putchar(49);
    }
    return putchar(48);
}

fn m1_bool() -> int {
    putchar(77); putchar(49); space();            // "M1 "   // "M1:"
    putchar(98); putchar(111); putchar(111); putchar(108); putchar(58); space();
    digit(2 < 3);
    digit(3 <= 2);
    digit(4 == 4);
    digit(5 != 5);
    digit(true && false);
    digit(true || false);
    digit(!true);
    return newline();
}

// ---------------------------------------------------------- M1: structs ----
// Values with named fields, passed and returned by value, nested freely.

struct Vec2 {
    x: int,
    y: int,
}

fn length_squared(v: Vec2) -> int {
    return v.x * v.x + v.y * v.y;
}

fn m1_struct() -> int {
    let v = Vec2 { x: 3, y: 4 };
    putchar(77); putchar(49); space();            // "M1 "
    putchar(115); putchar(116); putchar(114); putchar(117); putchar(99);
    putchar(116); putchar(58); space();
    putchar(40); print_nat(v.x); putchar(44); space(); print_nat(v.y); putchar(41);
    space(); putchar(45); putchar(62); space();
    print_nat(length_squared(v));               // 25
    return newline();
}

// ------------------------------------------------------------ M1: enums ----
// A value that is exactly one of several shapes. `match` must cover every
// variant, and the compiler names the ones you forgot.

enum Shape {
    Empty,
    Circle(int),
    Rect(int, int),
}

fn area(s: Shape) -> int {
    match s {
        Shape::Empty => { return 0; }
        Shape::Circle(r) => { return 3 * r * r; }
        Shape::Rect(w, h) => { return w * h; }
    }
}

fn m1_enum() -> int {
    putchar(77); putchar(49); space();            // "M1 "
    putchar(101); putchar(110); putchar(117); putchar(109); putchar(58); space();
    print_nat(area(Shape::Empty));              // 0
    space(); print_nat(area(Shape::Circle(2))); // 12
    space(); print_nat(area(Shape::Rect(4, 5)));// 20
    return newline();
}

// --------------------------------------------------------- M1: generics ----
// One definition, monomorphised per type it is used at. `Opt[T]` is the
// obvious first use; `T` is settled by the arguments at each call.

enum Opt[T] {
    None,
    Some(T),
}

fn unwrap_or[T](o: Opt[T], fallback: T) -> T {
    match o {
        Opt::None => { return fallback; }
        Opt::Some(value) => { return value; }
    }
}

fn m1_generic() -> int {
    putchar(77); putchar(49); space();            // "M1 "
    putchar(103); putchar(101); putchar(110); putchar(101); putchar(114);
    putchar(105); putchar(99); putchar(58); space();

    // Instantiated at `int` twice...
    print_nat(unwrap_or(Opt::Some(5), 0));      // 5
    let missing: Opt[int] = Opt::None;          // the annotation settles `T`
    space(); print_nat(unwrap_or(missing, 3));  // 3

    // ...and at `bool`, which is a second copy of the same source.
    space();
    if unwrap_or(Opt::Some(true), false) {
        putchar(122);                           // 'z'
    } else {
        putchar(45);
    }
    return newline();
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

fn issue(serial: int) -> Ticket {
    return Ticket { serial: serial };
}

// Takes ownership and hands it back, so the caller still owes one
// consumption -- ownership moved twice, not shared once.
fn stamp(t: Ticket) -> Ticket {
    let Ticket { serial } = t;
    return Ticket { serial: serial + 1 };
}

// The terminal consumer. Destructuring spends the whole and produces the
// parts; these parts are `int`, which is `val`, so nothing is owed after.
fn redeem(t: Ticket) -> int {
    let Ticket { serial } = t;
    return serial;
}

// `res` by inference: mode is structural, so an aggregate holding a `res`
// member is `res` without anyone writing the word.
struct Booking {
    outbound: Ticket,
    inbound: Ticket,
}

fn redeem_both(b: Booking) -> int {
    let Booking { outbound, inbound } = b;
    return redeem(outbound) + redeem(inbound);
}

// Both paths consume, so they agree about what is live at the merge point.
// An `if` whose `else` did not consume is refused rather than fixed up with a
// runtime drop flag -- see `tests/reject/branches_disagree.ls`.
fn redeem_either(t: Ticket, as_is: bool) -> int {
    if as_is {
        return redeem(t);
    }
    return redeem(stamp(t));
}

fn m2_linear() -> int {
    putchar(77); putchar(50); space();          // "M2 "
    putchar(108); putchar(105); putchar(110); putchar(101); putchar(97);
    putchar(114); putchar(58); space();         // "linear: "

    print_nat(redeem(issue(4)));                                    // 4
    space(); print_nat(redeem(stamp(issue(6))));                    // 7
    space(); print_nat(redeem_both(Booking {
        outbound: issue(2),
        inbound: issue(7),
    }));                                                            // 9
    space(); print_nat(redeem_either(issue(5), true));              // 5
    space(); print_nat(redeem_either(issue(5), false));             // 6
    return newline();
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
fn serial_of[&r](t: &r Ticket) -> int {
    return t.serial;
}

// `src <= dst` says `dst` outlives `src` (§5.2). Checking it is a walk up the
// stack of enclosing blocks -- O(depth), no fixpoint, total.
fn later_of[&dst, &src where src <= dst](a: &dst Ticket, b: &src Ticket) -> int {
    return serial_of(a) + serial_of(b);
}

fn m2_borrow() -> int {
    putchar(77); putchar(50); space();          // "M2 "
    putchar(98); putchar(111); putchar(114); putchar(114); putchar(111);
    putchar(119); putchar(58); space();         // "borrow: "

    let held = issue(4);

    borrow held as &r in {
        // Reading through the reference, which the owned value refuses.
        print_nat(r.serial);

        // Shared borrows nest: freezing is not exclusive, because two
        // readers neither move the value nor change it.
        space();
        borrow held as &inner in {
            print_nat(serial_of(r) + serial_of(inner));
        }

        // `r` comes from the enclosing block, so it outlives `inner` and may
        // be used where `&inner` is expected. Nothing else coerces.
        space();
        borrow held as &inner in {
            print_nat(later_of(r, inner) + serial_of(r));
        }
    }

    // Owned again, and still owed exactly one consumption.
    let spent = redeem(held);
    return newline();
}

fn main() -> int {
    m0();
    m1_bool();
    m1_struct();
    m1_enum();
    m1_generic();
    m2_linear();
    m2_borrow();
    return 0;
}
