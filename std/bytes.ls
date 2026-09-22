module std.bytes;

// `std.bytes` — text, which here means bytes.
//
// `docs/strings.md` §1: a string is **bytes, not an encoding**. So this
// is a byte module, and every function in it is honest about working on
// one byte or a run of them. There is no locale, no decoder and no
// notion of a character wider than eight bits, because the language has
// none of those and a library that pretended otherwise would be lying
// about what it can do.
//
// Classification takes an `int` rather than a `byte` because that is
// what `getchar` hands back and what `int_of` produces, and because
// `-1` — end of input — has to be classifiable as "not a digit" rather
// than trapping on the way in (`docs/standard-input.md` §3.1).

// Space, tab, newline, carriage return, vertical tab, form feed:
// exactly C's `isspace` in the C locale.
//
// This is the definition `examples/tally.ls` got wrong twice before it
// was written down once, which is most of what a standard library is
// for (`docs/standard-library.md` §5.3).
pub fn is_blank(c: int) -> [] bool {
    // 11 and 12 are the vertical tab and the form feed, and they stay
    // numbers: the escape set is `strings.md` §4's six and neither has
    // one (`docs/character-literals.md` §4). Before this line could name
    // the other four, all six were numbers and the sixth was either
    // correct or a typo for something else with no way to tell.
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == 11 || c == 12;
}

pub fn is_digit(c: int) -> [] bool {
    return c >= '0' && c <= '9';
}

pub fn is_upper(c: int) -> [] bool {
    return c >= 'A' && c <= 'Z';
}

pub fn is_lower(c: int) -> [] bool {
    return c >= 'a' && c <= 'z';
}

pub fn is_alpha(c: int) -> [] bool {
    return is_upper(c) || is_lower(c);
}

// ASCII case, and a no-op on everything else — including `-1`, which is
// what makes it safe to fold a byte before knowing whether input ended.
pub fn to_lower(c: int) -> [] int {
    if is_upper(c) {
        return c + 32;
    }
    return c;
}

pub fn to_upper(c: int) -> [] int {
    if is_lower(c) {
        return c - 32;
    }
    return c;
}

// The value a digit byte stands for, or `-1`.
//
// `-1` rather than an enum, matching `fs_read` and `getchar`. The same
// caveat applies and the same question is open
// (`docs/standard-input.md` §6).
pub fn digit_of(c: int) -> [] int {
    if is_digit(c) {
        return c - 48;
    }
    return 0 - 1;
}

pub fn equal[&a, &b](x: &a [byte], y: &b [byte]) -> [] bool {
    if len(x) != len(y) {
        return false;
    }
    var i = 0;
    while i < len(x) {
        if x[i] != y[i] {
            return false;
        }
        i = i + 1;
    }
    return true;
}

// Is `prefix` at the front of `text`? An empty prefix is, which is the
// answer every other definition of "starts with" gives and the one that
// makes `find` below terminate.
pub fn starts_with[&t, &p](text: &t [byte], prefix: &p [byte]) -> [] bool {
    if len(prefix) > len(text) {
        return false;
    }
    // One line, and it used to be a loop. `docs/slicing.md`: this
    // function existed *because* `text[0..len(prefix)]` could not be
    // written, so it is the clearest measure of what slicing bought.
    return equal(text[0..len(prefix)], prefix);
}

pub fn ends_with[&t, &p](text: &t [byte], suffix: &p [byte]) -> [] bool {
    if len(suffix) > len(text) {
        return false;
    }
    return equal(text[len(text) - len(suffix)..len(text)], suffix);
}

pub fn find[&t, &n](text: &t [byte], needle: &n [byte]) -> [] int {
    if len(needle) > len(text) {
        return 0 - 1;
    }
    var at = 0;
    while at + len(needle) <= len(text) {
        var i = 0;
        var same = true;
        while i < len(needle) && same {
            if text[at + i] != needle[i] {
                same = false;
            }
            i = i + 1;
        }
        if same {
            return at;
        }
        at = at + 1;
    }
    return 0 - 1;
}

// How many times the byte `b` occurs in `text`.
//
// Takes an `int` for the same reason the classifiers do: it is what
// `int_of` and `getchar` produce, and a caller that has one should not
// have to build a `byte` to ask a question about it.
//
// `examples/cut/` is why this exists: the number of fields in a line is
// the number of delimiters plus one, and counting them was the first
// thing that program wrote by hand.
pub fn count_byte[&t](text: &t [byte], b: int) -> [] int {
    var n = 0;
    var at = 0;
    while at < len(text) {
        if int_of(text[at]) == b {
            n = n + 1;
        }
        at = at + 1;
    }
    return n;
}

// Where the `n`-th field of `text` begins and ends, splitting on `b`.
//
// Fields are **1-based**, because that is how `cut` counts them and
// this function exists for `cut`. Asking for a field past the end
// answers an empty slice rather than trapping: a short line is an
// ordinary thing for a cutter to meet, not a bug in the program.
//
// The slice comes back bound to `&t`, so a field cannot outlive the
// line it was cut from — the escape check does that, and it is the same
// property `slicing.md` §1 gave every other subslice.
pub fn field[&t](text: &t [byte], b: int, n: int) -> [] &t [byte] {
    if n < 1 {
        return text[0..0];
    }
    var at = 0;
    var seen = 1;
    while seen < n && at < len(text) {
        if int_of(text[at]) == b {
            seen = seen + 1;
        }
        at = at + 1;
    }
    if seen < n {
        return text[0..0];
    }
    var end = at;
    while end < len(text) && int_of(text[end]) != b {
        end = end + 1;
    }
    return text[at..end];
}

// `text` without leading or trailing blanks (`is_blank`).
//
// Bound to `&t` like `field`, and for the same reason. Named `trim`
// rather than `strip` because every tool this library is measured
// against spells it that way.
pub fn trim[&t](text: &t [byte]) -> [] &t [byte] {
    var at = 0;
    while at < len(text) && is_blank(int_of(text[at])) {
        at = at + 1;
    }
    var end = len(text);
    while end > at && is_blank(int_of(text[end - 1])) {
        end = end - 1;
    }
    return text[at..end];
}

// Byte order: negative when `a` sorts first, 0 when they are equal,
// positive when `b` does. Shorter first when one is a prefix of the
// other.
//
// This is `LC_ALL=C` and nothing else, because the language has no
// locale to implement anything else — and `utf8.md` §5 records that
// byte order and code-point order agree for UTF-8, so this sorts text
// correctly without decoding it.
//
// `examples/sort/` had this inline, over `(text, at, len)` triples
// rather than slices, because it sorts indices into one buffer. It
// still does; what changed is that the *rule* is written down once.
pub fn compare[&a, &b](a: &a [byte], b: &b [byte]) -> [] int {
    var i = 0;
    while i < len(a) && i < len(b) {
        let x = int_of(a[i]);
        let y = int_of(b[i]);
        if x != y {
            return x - y;
        }
        i = i + 1;
    }
    return len(a) - len(b);
}
