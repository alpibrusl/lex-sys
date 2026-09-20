// rational.ls — M1's acceptance program (#1).
//
// Exact rational arithmetic: a `Rational` struct, a generic `Result[T]` for
// operations that can fail, an `Ordering` enum, and exhaustive matching
// throughout. Roughly 200 lines, and it is the honest test of whether M1's
// four pieces — types, structs, enums with pattern matching, and monomorphised
// generics — actually compose.
//
// What it cannot be is a tree. An expression evaluator is the classic program
// to write here, and M1 cannot express one: an `enum Expr { Add(Expr, Expr) }`
// contains itself, and with no references that has no finite size. The
// language gets pointers in M2 and that program becomes writable. This is the
// most real thing M1 can run, not a sketch of one.
//
//~ STDOUT 3/4 -1/3
//~ STDOUT 5/6 1/6 1/6 3/2
//~ STDOUT D Z
//~ STDOUT YN 7 T
//~ STDOUT <=>
//~ STDOUT 49/20
//~ EXIT 0

// ---------------------------------------------------------------- output ---

fn newline[&i](io: &!i Io) -> [io] int {
    return putchar(io, 10);
}

fn space[&i](io: &!i Io) -> [io] int {
    return putchar(io, 32);
}

fn print_digit[&i](io: &!i Io, d: int) -> [io] int {
    return putchar(io, 48 + d);
}

// Recursive, so the most significant digit is written first.
fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return print_digit(io, n % 10);
}

fn print_int[&i](io: &!i Io, n: int) -> [io] int {
    if n < 0 {
        putchar(io, 45);
        return print_nat(io, 0 - n);
    }
    return print_nat(io, n);
}

// ------------------------------------------------------------- the types ---

enum Error {
    DivideByZero,
    ZeroDenominator,
}

// The generic carried through every fallible operation below.
enum Result[T] {
    Ok(T),
    Err(Error),
}

enum Ordering {
    Less,
    Equal,
    Greater,
}

struct Rational {
    num: int,
    den: int,
}

// -------------------------------------------------- generic Result helpers ---

fn is_ok[T](r: Result[T]) -> [] bool {
    match r {
        Result::Ok(_) => { return true; }
        Result::Err(_) => { return false; }
    }
}

fn unwrap_or[T](r: Result[T], fallback: T) -> [] T {
    match r {
        Result::Ok(value) => { return value; }
        Result::Err(_) => { return fallback; }
    }
}

// ------------------------------------------------------------ arithmetic ---

fn abs(x: int) -> [] int {
    if x < 0 {
        return 0 - x;
    }
    return x;
}

fn gcd(a: int, b: int) -> [] int {
    if b == 0 {
        return abs(a);
    }
    return gcd(b, a % b);
}

// The one place a `Rational` is built, so every one that exists is normalised:
// denominator positive, terms in lowest form.
fn rational(num: int, den: int) -> [] Result[Rational] {
    if den == 0 {
        return Result::Err(Error::ZeroDenominator);
    }

    var n = num;
    var d = den;
    if d < 0 {
        n = 0 - n;
        d = 0 - d;
    }

    let divisor = gcd(n, d);
    if divisor == 0 {
        // n and d are both zero, which `den == 0` already ruled out.
        return Result::Err(Error::ZeroDenominator);
    }
    return Result::Ok(Rational { num: n / divisor, den: d / divisor });
}

fn add(a: Rational, b: Rational) -> [] Result[Rational] {
    return rational(a.num * b.den + b.num * a.den, a.den * b.den);
}

fn sub(a: Rational, b: Rational) -> [] Result[Rational] {
    return rational(a.num * b.den - b.num * a.den, a.den * b.den);
}

fn mul(a: Rational, b: Rational) -> [] Result[Rational] {
    return rational(a.num * b.num, a.den * b.den);
}

fn div(a: Rational, b: Rational) -> [] Result[Rational] {
    if b.num == 0 {
        return Result::Err(Error::DivideByZero);
    }
    return rational(a.num * b.den, a.den * b.num);
}

fn compare(a: Rational, b: Rational) -> [] Ordering {
    // Denominators are positive by construction, so cross-multiplying keeps
    // the direction of the comparison.
    let left = a.num * b.den;
    let right = b.num * a.den;
    if left < right {
        return Ordering::Less;
    }
    if left == right {
        return Ordering::Equal;
    }
    return Ordering::Greater;
}

// --------------------------------------------------------------- printing ---

fn print_rational[&i](io: &!i Io, r: Rational) -> [io] int {
    print_int(io, r.num);
    if r.den != 1 {
        putchar(io, 47);
        print_int(io, r.den);
    }
    return 0;
}

fn print_error[&i](io: &!i Io, e: Error) -> [io] int {
    match e {
        Error::DivideByZero => { return putchar(io, 68); }
        Error::ZeroDenominator => { return putchar(io, 90); }
    }
}

fn print_result[&i](io: &!i Io, r: Result[Rational]) -> [io] int {
    match r {
        Result::Ok(value) => { return print_rational(io, value); }
        Result::Err(e) => { return print_error(io, e); }
    }
}

fn print_ordering[&i](io: &!i Io, o: Ordering) -> [io] int {
    match o {
        Ordering::Less => { return putchar(io, 60); }
        Ordering::Equal => { return putchar(io, 61); }
        Ordering::Greater => { return putchar(io, 62); }
    }
}

// -------------------------------------------------------------------- demo ---

fn zero() -> [] Rational {
    return unwrap_or(rational(0, 1), Rational { num: 0, den: 1 });
}

fn one() -> [] Rational {
    return unwrap_or(rational(1, 1), Rational { num: 0, den: 1 });
}

// 1/1 + 1/2 + ... + 1/n, computed exactly.
fn harmonic(n: int) -> [] Result[Rational] {
    var total = zero();
    var i = 1;
    while i <= n {
        let term = unwrap_or(rational(1, i), zero());
        total = unwrap_or(add(total, term), total);
        i = i + 1;
    }
    return Result::Ok(total);
}

fn run[&i](io: &!i Io) -> [io] int {
    // Normalisation: 6/8 is 3/4, and a negative denominator moves to the top.
    print_result(io, rational(6, 8));
    space(io);
    print_result(io, rational(3, -9));
    newline(io);

    let half = unwrap_or(rational(1, 2), zero());
    let third = unwrap_or(rational(1, 3), zero());

    print_result(io, add(half, third));      // 5/6
    space(io);
    print_result(io, sub(half, third));      // 1/6
    space(io);
    print_result(io, mul(half, third));      // 1/6
    space(io);
    print_result(io, div(half, third));      // 3/2
    newline(io);

    // The failures, reported rather than trapped.
    print_result(io, div(half, zero()));     // D
    space(io);
    print_result(io, rational(1, 0));        // Z
    newline(io);

    // `is_ok` and `unwrap_or` at two different instantiations.
    if is_ok(rational(1, 2)) { putchar(io, 89); } else { putchar(io, 78); }
    if is_ok(div(one(), zero())) { putchar(io, 89); } else { putchar(io, 78); }
    space(io);
    print_int(io, unwrap_or(Result::Ok(7), 0));
    space(io);
    if unwrap_or(Result::Err(Error::DivideByZero), true) { putchar(io, 84); } else { putchar(io, 70); }
    newline(io);

    // Ordering, matched exhaustively.
    print_ordering(io, compare(third, half));
    print_ordering(io, compare(half, half));
    print_ordering(io, compare(half, third));
    newline(io);

    // H(6) = 49/20.
    print_result(io, harmonic(6));
    newline(io);
    return 0;
}

fn main(world: World) -> [] int {
    // §8.2: the runtime hands over exactly one `World`, and `split` consumes
    // it. There is no other way to obtain a capability.
    let Split { io } = split(world);
    var status = 0;
    // Threaded by borrow, not by move: a callee should not consume its
    // caller's authority.
    borrow mut io as &!i in {
        status = run(i);
    }
    // Authority is a resource, so it is destroyed exactly once. A program
    // that forgets this does not compile.
    release(io);
    return status;
}
