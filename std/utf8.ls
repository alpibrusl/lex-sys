module std.utf8;

// `std.utf8` — decoding a run of bytes into code points.
//
// `docs/utf8.md` is the design and §2 is the part worth reading before
// this file: `docs/strings.md` §1 declined a validated string type
// because such a type must say what an invalid one *is*, and concluded
// that bytes "answer it by not asking". That is right for the
// representation and unavailable to a decoder, which is handed `c0 80`
// and has to do something.
//
// Four implementations give four different answers on malformed input
// (§2's table). This one is strict about what is valid and reports what
// it found, and §3 is why it neither traps nor silently substitutes.

// One step of a decode.
//
// Not a `Result`: the caller needs the width on the *failing* path too,
// or it cannot advance and the loop does not terminate. That is the
// whole reason this carries a number in both arms.
pub enum Step {
    // The code point, and how many bytes it took (1 to 4).
    Code(int, int),
    // How many bytes to skip. Never 0, so a loop always advances.
    Invalid(int),
}

// A continuation byte is `10xxxxxx`.
fn is_tail(b: int) -> [] bool {
    return (b & 0xc0) == 0x80;
}

// Whether `b` is a continuation byte in `[lo, hi]`.
//
// The range is not always `0x80..0xbf`: `docs/utf8.md` §3.1 refuses
// overlong forms, surrogates and anything above U+10FFFF, and each of
// those three is a constraint on the *second* byte alone. Table 3-7 of
// the Unicode standard is where the four narrowed ranges come from, and
// checking them here is what makes the rest of the sequence ordinary.
fn in_range(b: int, lo: int, hi: int) -> [] bool {
    return b >= lo && b <= hi;
}

// The allowed range of the second byte, given the first.
//
// Answers `(lo, hi)`, or `(1, 0)` — an empty range — when the first byte
// cannot begin a multi-byte sequence at all.
fn second_range(first: int) -> [] (int, int) {
    if in_range(first, 0xc2, 0xdf) { return (0x80, 0xbf); }
    // `e0 80..9f` would be an overlong three-byte form.
    if first == 0xe0 { return (0xa0, 0xbf); }
    if in_range(first, 0xe1, 0xec) { return (0x80, 0xbf); }
    // `ed a0..bf` is the UTF-16 surrogate block, which is not a scalar
    // value and has no UTF-8 encoding.
    if first == 0xed { return (0x80, 0x9f); }
    if in_range(first, 0xee, 0xef) { return (0x80, 0xbf); }
    // `f0 80..8f` would be an overlong four-byte form.
    if first == 0xf0 { return (0x90, 0xbf); }
    if in_range(first, 0xf1, 0xf3) { return (0x80, 0xbf); }
    // `f4 90..bf` would be above U+10FFFF.
    if first == 0xf4 { return (0x80, 0x8f); }
    return (1, 0);
}

// How many bytes a sequence beginning with `first` claims.
fn width_of(first: int) -> [] int {
    if first < 0x80 { return 1; }
    if in_range(first, 0xc2, 0xdf) { return 2; }
    if in_range(first, 0xe0, 0xef) { return 3; }
    if in_range(first, 0xf0, 0xf4) { return 4; }
    // `80..bf` is a lone continuation; `c0`, `c1` are always overlong;
    // `f5` and above are always out of range.
    return 0;
}

// How many low bits of the first byte belong to the code point.
fn lead_bits(width: int) -> [] int {
    if width == 2 { return 0x1f; }
    if width == 3 { return 0x0f; }
    return 0x07;
}

// One step, starting at `at`.
//
// `at` must be within `text`; a caller that has run off the end should
// stop rather than ask. The **maximal subpart** rule (§3.2) governs how
// far `Invalid` skips: as many bytes as could still have begun a
// well-formed sequence, and never fewer than one. That is why a
// truncated `e2 82` is one `Invalid(2)` while `c0 80` is two
// `Invalid(1)`s — `c0` could never have begun anything.
pub fn decode[&r](text: &r [byte], at: int) -> [] Step {
    let first = int_of(text[at]);
    let width = width_of(first);
    if width == 0 {
        return Step::Invalid(1);
    }
    if width == 1 {
        return Step::Code(first, 1);
    }

    // The second byte carries the whole validity question (§3.1), so it
    // is checked against its own range before the rest are checked as
    // ordinary continuations.
    if at + 1 >= len(text) {
        return Step::Invalid(1);
    }
    let range = second_range(first);
    let second = int_of(text[at + 1]);
    if !in_range(second, range.0, range.1) {
        return Step::Invalid(1);
    }

    var point = (first & lead_bits(width)) << 6 | (second & 0x3f);
    var k = 2;
    while k < width {
        // Ran out of text, or a byte that is not a continuation: the
        // bytes already consumed were a plausible beginning, so they are
        // one error rather than several.
        if at + k >= len(text) {
            return Step::Invalid(k);
        }
        let b = int_of(text[at + k]);
        if !is_tail(b) {
            return Step::Invalid(k);
        }
        point = point << 6 | (b & 0x3f);
        k = k + 1;
    }
    return Step::Code(point, width);
}

// How many code points are in `text`, counting each malformed run as
// one.
//
// That last clause is a choice and not the only one — §2's table is four
// implementations disagreeing about it. Counting a malformed run as one
// is what makes this agree with `wc -m` on well-formed input while still
// terminating on anything.
pub fn count[&r](text: &r [byte]) -> [] int {
    var n = 0;
    var at = 0;
    while at < len(text) {
        match decode(text, at) {
            Step::Code(_, width) => { at = at + width; }
            Step::Invalid(width) => { at = at + width; }
        }
        n = n + 1;
    }
    return n;
}

// Whether every byte of `text` is part of a well-formed sequence.
pub fn is_valid[&r](text: &r [byte]) -> [] bool {
    var at = 0;
    while at < len(text) {
        match decode(text, at) {
            Step::Code(_, width) => { at = at + width; }
            Step::Invalid(_) => { return false; }
        }
    }
    return true;
}
