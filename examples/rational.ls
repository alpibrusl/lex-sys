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

fn newline() -> int {
    return putchar(10);
}

fn space() -> int {
    return putchar(32);
}

fn print_digit(d: int) -> int {
    return putchar(48 + d);
}

// Recursive, so the most significant digit is written first.
fn print_nat(n: int) -> int {
    if n >= 10 {
        print_nat(n / 10);
    }
    return print_digit(n % 10);
}

fn print_int(n: int) -> int {
    if n < 0 {
        putchar(45);
        return print_nat(0 - n);
    }
    return print_nat(n);
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

fn is_ok[T](r: Result[T]) -> bool {
    match r {
        Result::Ok(_) => { return true; }
        Result::Err(_) => { return false; }
    }
}

fn unwrap_or[T](r: Result[T], fallback: T) -> T {
    match r {
        Result::Ok(value) => { return value; }
        Result::Err(_) => { return fallback; }
    }
}

// ------------------------------------------------------------ arithmetic ---

fn abs(x: int) -> int {
    if x < 0 {
        return 0 - x;
    }
    return x;
}

fn gcd(a: int, b: int) -> int {
    if b == 0 {
        return abs(a);
    }
    return gcd(b, a % b);
}

// The one place a `Rational` is built, so every one that exists is normalised:
// denominator positive, terms in lowest form.
fn rational(num: int, den: int) -> Result[Rational] {
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

fn add(a: Rational, b: Rational) -> Result[Rational] {
    return rational(a.num * b.den + b.num * a.den, a.den * b.den);
}

fn sub(a: Rational, b: Rational) -> Result[Rational] {
    return rational(a.num * b.den - b.num * a.den, a.den * b.den);
}

fn mul(a: Rational, b: Rational) -> Result[Rational] {
    return rational(a.num * b.num, a.den * b.den);
}

fn div(a: Rational, b: Rational) -> Result[Rational] {
    if b.num == 0 {
        return Result::Err(Error::DivideByZero);
    }
    return rational(a.num * b.den, a.den * b.num);
}

fn compare(a: Rational, b: Rational) -> Ordering {
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

fn print_rational(r: Rational) -> int {
    print_int(r.num);
    if r.den != 1 {
        putchar(47);
        print_int(r.den);
    }
    return 0;
}

fn print_error(e: Error) -> int {
    match e {
        Error::DivideByZero => { return putchar(68); }
        Error::ZeroDenominator => { return putchar(90); }
    }
}

fn print_result(r: Result[Rational]) -> int {
    match r {
        Result::Ok(value) => { return print_rational(value); }
        Result::Err(e) => { return print_error(e); }
    }
}

fn print_ordering(o: Ordering) -> int {
    match o {
        Ordering::Less => { return putchar(60); }
        Ordering::Equal => { return putchar(61); }
        Ordering::Greater => { return putchar(62); }
    }
}

// -------------------------------------------------------------------- demo ---

fn zero() -> Rational {
    return unwrap_or(rational(0, 1), Rational { num: 0, den: 1 });
}

fn one() -> Rational {
    return unwrap_or(rational(1, 1), Rational { num: 0, den: 1 });
}

// 1/1 + 1/2 + ... + 1/n, computed exactly.
fn harmonic(n: int) -> Result[Rational] {
    var total = zero();
    var i = 1;
    while i <= n {
        let term = unwrap_or(rational(1, i), zero());
        total = unwrap_or(add(total, term), total);
        i = i + 1;
    }
    return Result::Ok(total);
}

fn main() -> int {
    // Normalisation: 6/8 is 3/4, and a negative denominator moves to the top.
    print_result(rational(6, 8));
    space();
    print_result(rational(3, -9));
    newline();

    let half = unwrap_or(rational(1, 2), zero());
    let third = unwrap_or(rational(1, 3), zero());

    print_result(add(half, third));      // 5/6
    space();
    print_result(sub(half, third));      // 1/6
    space();
    print_result(mul(half, third));      // 1/6
    space();
    print_result(div(half, third));      // 3/2
    newline();

    // The failures, reported rather than trapped.
    print_result(div(half, zero()));     // D
    space();
    print_result(rational(1, 0));        // Z
    newline();

    // `is_ok` and `unwrap_or` at two different instantiations.
    if is_ok(rational(1, 2)) { putchar(89); } else { putchar(78); }
    if is_ok(div(one(), zero())) { putchar(89); } else { putchar(78); }
    space();
    print_int(unwrap_or(Result::Ok(7), 0));
    space();
    if unwrap_or(Result::Err(Error::DivideByZero), true) { putchar(84); } else { putchar(70); }
    newline();

    // Ordering, matched exhaustively.
    print_ordering(compare(third, half));
    print_ordering(compare(half, half));
    print_ordering(compare(half, third));
    newline();

    // H(6) = 49/20.
    print_result(harmonic(6));
    newline();
    return 0;
}
