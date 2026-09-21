module std.bignum;

// `std.bignum` — exact integers wider than `int`, only as wide as
// `std.fmt` needs and no wider.
//
// A number is a `[int]` whose elements are **base 2^32 limbs**, least
// significant first, and whose length never changes: operations write
// through a unique reference and leading zeros are ordinary. That makes
// every routine here allocation-free, which is what lets the caller keep
// five of them in one arena (`float-printing.md` §6).
//
// Base 2^32 rather than 2^63 because a limb has to survive being
// multiplied by ten: 10 * (2^32 - 1) is about 2^35.7, comfortably inside
// `int`, where the same product in base 2^63 would trap.
//
// There is no division. `std.fmt` needs one quotient, it is a single
// digit, and nine subtractions settle it — which is why this module can
// be ninety lines instead of four hundred (`float-printing.md` §3.2).

// 2^32, the base.
pub fn base() -> [] int {
    return 4294967296;
}

pub fn zero[&a](a: &!a [int]) -> [] int {
    var i = 0;
    while i < len(a) {
        a[i] = 0;
        i = i + 1;
    }
    return 0;
}

// `a = value`, for a value that may be up to 2^53 and so may not fit in
// one limb.
pub fn set[&a](a: &!a [int], value: int) -> [] int {
    zero(a);
    a[0] = value & 0xffffffff;
    a[1] = (value >> 32) & 0xffffffff;
    return 0;
}

// `a = a * multiplier`, for a small multiplier — ten and two are the
// only ones used.
pub fn mul_small[&a](a: &!a [int], multiplier: int) -> [] int {
    var carry = 0;
    var i = 0;
    while i < len(a) {
        let product = a[i] * multiplier + carry;
        a[i] = product & 0xffffffff;
        carry = product >> 32;
        i = i + 1;
    }
    return carry;
}

// `a = a << places`. Done as a limb move and then a bit shift, rather
// than by doubling `places` times: the exponents here reach 1075.
pub fn shift_left[&a](a: &!a [int], places: int) -> [] int {
    let whole = places / 32;
    let bits = places - whole * 32;

    if whole > 0 {
        var i = len(a) - 1;
        while i >= 0 {
            if i >= whole {
                a[i] = a[i - whole];
            } else {
                a[i] = 0;
            }
            i = i - 1;
        }
    }

    if bits > 0 {
        var carry = 0;
        var i = 0;
        while i < len(a) {
            let shifted = (a[i] << bits) | carry;
            a[i] = shifted & 0xffffffff;
            carry = shifted >> 32;
            i = i + 1;
        }
    }
    return 0;
}

// -1, 0 or 1 as `a` is less than, equal to or greater than `b`.
pub fn compare[&a, &b](a: &a [int], b: &b [int]) -> [] int {
    var i = len(a) - 1;
    while i >= 0 {
        if a[i] != b[i] {
            if a[i] < b[i] {
                return 0 - 1;
            }
            return 1;
        }
        i = i - 1;
    }
    return 0;
}

// `a = a - b`, which the caller has established does not go negative.
pub fn subtract[&a, &b](a: &!a [int], b: &b [int]) -> [] int {
    // Not called `borrow`, which is a keyword here for the other kind.
    var owed = 0;
    var i = 0;
    while i < len(a) {
        var digit = a[i] - b[i] - owed;
        if digit < 0 {
            digit = digit + base();
            owed = 1;
        } else {
            owed = 0;
        }
        a[i] = digit;
        i = i + 1;
    }
    return owed;
}

// `into = a + b`. The one operation that needs somewhere to put its
// answer, because every caller wants to compare the sum rather than keep
// it.
pub fn add_into[&d, &a, &b](into: &!d [int], a: &a [int], b: &b [int]) -> [] int {
    var carry = 0;
    var i = 0;
    while i < len(into) {
        let sum = a[i] + b[i] + carry;
        into[i] = sum & 0xffffffff;
        carry = sum >> 32;
        i = i + 1;
    }
    return carry;
}

// `into = a`.
pub fn copy[&d, &a](into: &!d [int], a: &a [int]) -> [] int {
    var i = 0;
    while i < len(into) {
        into[i] = a[i];
        i = i + 1;
    }
    return 0;
}
