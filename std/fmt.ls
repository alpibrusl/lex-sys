module std.fmt;

// `std.fmt` — printing a `float` so that reading it back gives the same
// bits, in the fewest digits for which that is true.
//
// `floating-point.md` §7 called this the thing that makes `float`
// awkward rather than incomplete, and it is the reason: without it a
// program reports numbers through `truncate` and a scale factor, which
// is fixed point with extra steps.
//
// The algorithm is Steele and White's, the one Dragon4 is a refinement
// of: keep the value as an exact ratio of integers, keep the distance to
// each neighbouring float alongside it, and emit digits until what is
// left is closer to this float than to either neighbour. The digits are
// then the shortest that round-trip, and they are *correct* rather than
// nearly always correct, which is the difference between this and Grisu2.
//
// **It is written here rather than in the compiler**, which is the point
// of `bits_of` (`float-printing.md` §2). The integers involved reach
// 1080 bits, so `std.bignum` is underneath — also written here.
//
// The one trick worth naming: a digit is `R / S` for bignums `R` and
// `S`, which sounds like it needs bignum division. It does not, because
// the digit is 0..9: nine subtractions settle it, and subtraction is
// easy where division is not (§3.2).

import std.bignum;

// How many limbs the exact arithmetic needs. The widest any of the five
// numbers gets is 1080 bits — 34 limbs, measured rather than bounded
// (§4), near the bottom of the subnormals where the numerator picks up
// 10^323 on the way to the first digit. 80 is more than double that, and
// in an arena the slack costs one bump of a pointer.
fn limbs() -> [] int {
    return 80;
}

// `x` decomposed as `m * 2^e`, exactly. The mantissa carries the
// implicit bit for a normal number and does not for a subnormal, which
// is the whole of the difference between the two.
fn mantissa(bits: int) -> [] int {
    let raw = bits & 0xfffffffffffff;
    let exponent = (bits >> 52) & 0x7ff;
    if exponent == 0 {
        return raw;
    }
    return raw | (1 << 52);
}

fn exponent_of(bits: int) -> [] int {
    let exponent = (bits >> 52) & 0x7ff;
    if exponent == 0 {
        // Subnormal: the exponent is the same as the smallest normal's,
        // and it is the mantissa that lost its implicit bit.
        return 0 - 1074;
    }
    return exponent - 1075;
}

// Is this a power of two with a neighbour below that is *closer* than
// the one above? True exactly when the mantissa is the implicit bit
// alone and the exponent is not the smallest -- the boundary case every
// shortest-printing paper spends a paragraph on.
fn uneven_neighbours(bits: int) -> [] bool {
    let raw = bits & 0xfffffffffffff;
    let exponent = (bits >> 52) & 0x7ff;
    return raw == 0 && exponent > 1;
}

// Write `x` into `out` as the shortest decimal that reads back to the
// same bits, and answer how many bytes that took.
//
// The form is `d[.ddd]e[-]k` — one digit, the rest, and the exponent —
// which is what the algorithm produces and what an oracle can be
// compared against without a second set of decisions about when to use
// positional notation (§1.1). `0.1` comes out as `1e-1`.
//
// Answers -1 if `out` is too short. 24 bytes is always enough: a sign,
// seventeen digits, a point, `e`, a sign and three exponent digits.
pub fn float_into[&o](out: &!o [byte], x: float) -> [] int {
    if is_nan(x) {
        return put(out, 0, "NaN");
    }
    let bits = bits_of(x);
    let negative = bits < 0;
    var at = 0;
    if negative {
        at = put(out, 0, "-");
        if at < 0 {
            return 0 - 1;
        }
    }

    // Infinity is the one exponent with an empty mantissa reserved for
    // it, and `floating-point.md` §2 says it is a value rather than an
    // error, so it gets spelled rather than refused.
    if (bits & 0x7fffffffffffffff) == 0x7ff0000000000000 {
        return put(out, at, "inf");
    }
    if (bits & 0x7fffffffffffffff) == 0 {
        return put(out, at, "0e0");
    }

    var written = 0 - 1;
    region a {
        // Five numbers and the digits. `limbs()` is the worst case, so
        // the same arena serves every value.
        let r = alloc_slice[a](limbs(), 0);
        let s = alloc_slice[a](limbs(), 0);
        let minus = alloc_slice[a](limbs(), 0);
        let plus = alloc_slice[a](limbs(), 0);
        let scratch = alloc_slice[a](limbs(), 0);
        let digits = alloc_slice[a](24, 0);

        let m = mantissa(bits);
        let e = exponent_of(bits);
        let uneven = uneven_neighbours(bits);
        // Round-to-nearest-**even** makes the boundaries inclusive for an
        // even mantissa: a decimal exactly halfway to the neighbour still
        // reads back to this float, because the tie goes to the even one.
        // So every comparison against a boundary below is `<=` here and
        // `<` otherwise -- four places, and skipping them costs a digit
        // (§3.3).
        let even = (m & 1) == 0;

        // The value is `r / s`, and the midpoints to the neighbour above
        // and below are `plus / s` and `minus / s`. Scaled so every one
        // of them is an integer, which is the whole reason for the
        // factors of two (§3.1).
        if e >= 0 {
            bignum.set(r, m);
            bignum.set(minus, 1);
            bignum.set(plus, 1);
            if uneven {
                bignum.shift_left(r, e + 2);
                bignum.set(s, 4);
                bignum.shift_left(minus, e);
                bignum.shift_left(plus, e + 1);
            } else {
                bignum.shift_left(r, e + 1);
                bignum.set(s, 2);
                bignum.shift_left(minus, e);
                bignum.shift_left(plus, e);
            }
        } else {
            bignum.set(r, m);
            bignum.set(s, 1);
            bignum.set(minus, 1);
            if uneven {
                bignum.mul_small(r, 4);
                bignum.shift_left(s, 2 - e);
                bignum.set(plus, 2);
            } else {
                bignum.mul_small(r, 2);
                bignum.shift_left(s, 1 - e);
                bignum.set(plus, 1);
            }
        }

        // Scale so the first digit is the first digit: afterwards
        // `r + plus <= s` and `10 * (r + plus) > s`, so the value sits in
        // `[0.1, 1)` and `k` says where the point went.
        var k = 0;
        var settled = false;
        while !settled {
            bignum.add_into(scratch, r, plus);
            let over = bignum.compare(scratch, s);
            if over > 0 || (even && over == 0) {
                bignum.mul_small(s, 10);
                k = k + 1;
            } else {
                settled = true;
            }
        }
        settled = false;
        while !settled {
            bignum.add_into(scratch, r, plus);
            bignum.mul_small(scratch, 10);
            let under = bignum.compare(scratch, s);
            if under < 0 || (!even && under == 0) {
                bignum.mul_small(r, 10);
                bignum.mul_small(minus, 10);
                bignum.mul_small(plus, 10);
                k = k - 1;
            } else {
                settled = true;
            }
        }

        // Digits, until what is left is nearer this float than either
        // neighbour. That is the stopping rule, and it is what makes the
        // answer shortest rather than merely correct.
        var count = 0;
        var done = false;
        while !done && count < 20 {
            bignum.mul_small(r, 10);
            bignum.mul_small(minus, 10);
            bignum.mul_small(plus, 10);

            // The digit, by subtraction: it is 0..9, so nine of them
            // settle it and no bignum division is needed (§3.2).
            var digit = 0;
            while bignum.compare(r, s) >= 0 {
                bignum.subtract(r, s);
                digit = digit + 1;
            }

            let under = bignum.compare(r, minus);
            let low = under < 0 || (even && under == 0);
            bignum.add_into(scratch, r, plus);
            let over = bignum.compare(scratch, s);
            let high = over > 0 || (even && over == 0);

            if low || high {
                var last = digit;
                if low && high {
                    // Both neighbours are in reach, so the remainder
                    // decides: `2r` against `s` is "which half".
                    //
                    // Dead level is the one case where both answers are
                    // shortest *and* round-trip, so the rule is a choice
                    // rather than a derivation. Steele and White's is
                    // "round the tie up", and this follows it: Rust's
                    // `{:e}` agrees and Python's `repr` does not, which
                    // is a measured fact about the two rather than a bug
                    // in either (§3.4).
                    bignum.copy(scratch, r);
                    bignum.mul_small(scratch, 2);
                    if bignum.compare(scratch, s) >= 0 {
                        last = digit + 1;
                    }
                } else {
                    if high {
                        last = digit + 1;
                    }
                }
                digits[count] = last;
                count = count + 1;
                done = true;
            } else {
                digits[count] = digit;
                count = count + 1;
            }
        }

        // `0.d1d2 * 10^k` is `d1.d2 * 10^(k-1)`, which is the form.
        at = put_digit(out, at, digits[0]);
        if at >= 0 && count > 1 {
            at = put(out, at, ".");
            var i = 1;
            while i < count && at >= 0 {
                at = put_digit(out, at, digits[i]);
                i = i + 1;
            }
        }
        if at >= 0 {
            at = put(out, at, "e");
        }
        if at >= 0 {
            at = put_int(out, at, k - 1);
        }
        written = at;
    }
    return written;
}

// ---------------------------------------------------------------------
// Writing bytes
// ---------------------------------------------------------------------

// Copy `text` in at `at`, answering where the next byte goes, or -1 if
// it does not fit. Every writer here threads that -1 rather than
// trapping, because a short buffer is the caller's business.
fn put[&o, &t](out: &!o [byte], at: int, text: &t [byte]) -> [] int {
    if at < 0 || at + len(text) > len(out) {
        return 0 - 1;
    }
    var i = 0;
    while i < len(text) {
        out[at + i] = text[i];
        i = i + 1;
    }
    return at + len(text);
}

fn put_digit[&o](out: &!o [byte], at: int, digit: int) -> [] int {
    if at < 0 || at + 1 > len(out) {
        return 0 - 1;
    }
    out[at] = byte_of(48 + digit);
    return at + 1;
}

fn put_int[&o](out: &!o [byte], at: int, value: int) -> [] int {
    var here = at;
    var rest = value;
    if rest < 0 {
        here = put(out, here, "-");
        rest = 0 - rest;
    }
    if rest == 0 {
        return put_digit(out, here, 0);
    }
    // Backwards then reversed, which is what an integer costs without a
    // buffer to build in.
    var reversed = 0;
    var places = 0;
    while rest > 0 {
        reversed = reversed * 10 + rest - (rest / 10) * 10;
        rest = rest / 10;
        places = places + 1;
    }
    while places > 0 && here >= 0 {
        here = put_digit(out, here, reversed - (reversed / 10) * 10);
        reversed = reversed / 10;
        places = places - 1;
    }
    return here;
}
