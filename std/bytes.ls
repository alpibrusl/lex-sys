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
    return c == 32 || c == 9 || c == 10 || c == 13 || c == 11 || c == 12;
}

pub fn is_digit(c: int) -> [] bool {
    return c >= 48 && c <= 57;
}

pub fn is_upper(c: int) -> [] bool {
    return c >= 65 && c <= 90;
}

pub fn is_lower(c: int) -> [] bool {
    return c >= 97 && c <= 122;
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
    var i = 0;
    while i < len(prefix) {
        if text[i] != prefix[i] {
            return false;
        }
        i = i + 1;
    }
    return true;
}

// Where `needle` first occurs in `text`, or `-1`.
//
// The naive scan: no Boyer-Moore, no table. A better algorithm is a
// policy this library could choose later, and until a program here is
// slow because of this one it would be a guess.
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
