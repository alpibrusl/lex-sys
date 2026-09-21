// `wordfreq/text.ls` — the byte helpers, in a file of their own.
//
// Nothing here knows what a word count is. That is the point of
// `docs/many-files.md`: before it, every helper a program needed had to
// live in the same file as the program, so there was no such thing as a
// library. These four functions are one now.

fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

// A space, a tab or a newline. `==` on `byte` compares storage, which is
// allowed; `+` on one is arithmetic, which is not (`docs/strings.md` §2).
fn is_space(b: byte) -> [] bool {
    return b == byte_of(32) || b == byte_of(10) || b == byte_of(9) || b == byte_of(13);
}

// Are the two ranges of `text` the same bytes?
//
// A word is a start and a length into the document rather than a copy of
// it, so comparing two words is comparing two ranges. No allocation
// anywhere in this file.
fn same_word[&t](text: &t [byte], a: int, a_len: int, b: int, b_len: int) -> [] bool {
    if a_len != b_len {
        return false;
    }
    var n = 0;
    while n < a_len {
        if text[a + n] != text[b + n] {
            return false;
        }
        n = n + 1;
    }
    return true;
}

fn write_word[&t, &i](io: &!i Io, text: &t [byte], start: int, length: int) -> [io_write] int {
    var n = 0;
    while n < length {
        putchar(io, int_of(text[start + n]));
        n = n + 1;
    }
    return length;
}
