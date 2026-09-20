// wordcount.ls — `wc` over an embedded document, and the first program in
// this repo that is mostly *text processing* rather than a demonstration.
//
// It is here because strings are the point at which lex-sys stops being a
// language you can show and starts being one you can use. Everything it
// does is ordinary: a literal is a slice, a slice is a reference, bytes are
// compared and converted explicitly, the scratch buffer lives in an arena,
// and the console capability is threaded to the one place that prints.
//
// What it is *not* is Unicode-aware. `docs/strings.md` §1 claims no
// encoding, so this counts bytes and ASCII whitespace, and says so. A UTF-8
// decoder is library work over exactly this slice type.
//~ STDOUT the quick brown fox
//~ STDOUT jumps over the lazy dog
//~ STDOUT and then the fox rests
//~ STDOUT --
//~ STDOUT lines   3
//~ STDOUT words   14
//~ STDOUT bytes   67
//~ STDOUT longest 23
//~ STDOUT the     3
//~ EXIT 0

// ------------------------------------------------------------- output ----

fn space[&i](io: &!i Io) -> [io] int {
    return putchar(io, 32);
}

fn newline[&i](io: &!i Io) -> [io] int {
    return putchar(io, 10);
}

fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

fn print_nat[&i](io: &!i Io, n: int) -> [io] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

// A label, then the number right-aligned in a small field. Three calls to
// this is the whole report.
fn row[&r, &i](io: &!i Io, label: &r [byte], value: int, width: int) -> [io] int {
    write_all(io, label);
    var pad = width - len(label);
    while pad > 0 {
        space(io);
        pad = pad - 1;
    }
    print_nat(io, value);
    return newline(io);
}

// --------------------------------------------------------- the bytes -----
// Named rather than written inline, so the counts below are checkable by
// reading one place.

fn document() -> [] &static [byte] {
    return "the quick brown fox\njumps over the lazy dog\nand then the fox rests\n";
}

fn newline_byte() -> [] byte {
    return byte_of(10);
}

fn space_byte() -> [] byte {
    return byte_of(32);
}

// §2: comparing storage is not arithmetic, so `==` on bytes is allowed --
// and it is the only operator that is.
fn is_blank(b: byte) -> [] bool {
    return b == space_byte() || b == newline_byte();
}

// ---------------------------------------------------------- counting -----

fn count_lines[&r](text: &r [byte]) -> [] int {
    var lines = 0;
    var n = 0;
    while n < len(text) {
        if text[n] == newline_byte() {
            lines = lines + 1;
        }
        n = n + 1;
    }
    return lines;
}

// A word is a run of non-blank bytes, so the count is the number of times a
// blank is followed by something that is not one.
fn count_words[&r](text: &r [byte]) -> [] int {
    var words = 0;
    var inside = false;
    var n = 0;
    while n < len(text) {
        if is_blank(text[n]) {
            inside = false;
        } else {
            if !inside {
                words = words + 1;
            }
            inside = true;
        }
        n = n + 1;
    }
    return words;
}

fn longest_line[&r](text: &r [byte]) -> [] int {
    var longest = 0;
    var current = 0;
    var n = 0;
    while n < len(text) {
        if text[n] == newline_byte() {
            if current > longest {
                longest = current;
            }
            current = 0;
        } else {
            current = current + 1;
        }
        n = n + 1;
    }
    if current > longest {
        longest = current;
    }
    return longest;
}

// --------------------------------------------------------- searching -----
// Two slices compared a byte at a time, which is all a string comparison
// is when a string is bytes.

fn matches_at[&r, &n](text: &r [byte], needle: &n [byte], at: int) -> [] int {
    if at + len(needle) > len(text) {
        return 0;
    }
    var i = 0;
    while i < len(needle) {
        if !(text[at + i] == needle[i]) {
            return 0;
        }
        i = i + 1;
    }
    return 1;
}

// Whole-word occurrences: a match that is not glued to a letter on either
// side, which is what stops `the` matching inside `then`.
fn count_word[&r, &n](text: &r [byte], needle: &n [byte]) -> [] int {
    var found = 0;
    var at = 0;
    while at + len(needle) <= len(text) {
        if matches_at(text, needle, at) == 1 {
            var standalone = true;
            if at > 0 {
                if !is_blank(text[at - 1]) {
                    standalone = false;
                }
            }
            let after = at + len(needle);
            if after < len(text) {
                if !is_blank(text[after]) {
                    standalone = false;
                }
            }
            if standalone {
                found = found + 1;
            }
        }
        at = at + 1;
    }
    return found;
}

// ------------------------------------------------------------- report ----

fn report[&i](io: &!i Io) -> [io] int {
    let text = document();
    write_all(io, text);
    write_all(io, "--\n");

    let lines = count_lines(text);
    let words = count_words(text);
    let bytes = len(text);
    let longest = longest_line(text);

    // The scratch copy exists to show that a buffer and a literal are the
    // same type: `count_word` takes `&r [byte]` and neither caller has to
    // say which kind it is holding.
    var thes = 0;
    region a {
        let needle = alloc_slice[a](3, byte_of(116));   // 't'
        needle[1] = byte_of(104);                       // 'h'
        needle[2] = byte_of(101);                       // 'e'
        thes = count_word(text, needle);
    }

    row(io, "lines", lines, 8);
    row(io, "words", words, 8);
    row(io, "bytes", bytes, 8);
    row(io, "longest", longest, 8);
    row(io, "the", thes, 8);

    return lines * 100 + words;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    // This program reads no arguments, so that authority ends here.
    release(args);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    // This program reads no files and calls into no library: it counts what
    // it was compiled with, and the row on every frame below says so.
    release(ffi);

    var status = 0;
    borrow mut io as &!i in {
        status = report(i);
    }
    release(io);
    return status - 314;
}
