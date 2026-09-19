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

fn main() -> int {
    m0();
    m1_bool();
    m1_struct();
    m1_enum();
    m1_generic();
    return 0;
}
