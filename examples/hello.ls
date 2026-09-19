// hello.ls — the M0 smoke program (#3).
//
// M0 has one type, `int`. There are no strings, no arrays and no FFI yet, so
// the greeting travels as two packed 64-bit words, seven bytes each, and is
// unpacked a byte at a time. That is not how anyone will write lex-sys once M3
// lands slices and strings — it is how you write a program when the language
// is exactly integers, functions, arithmetic, `if`, `while` and local
// bindings, which is precisely what this milestone claims to have.

// Write the low seven bytes of `word`, least significant first.
fn put_word(word: int) -> int {
    var rest = word;
    var written = 0;
    while rest > 0 {
        putchar(rest % 256);
        rest = rest / 256;
        written = written + 1;
    }
    return written;
}

// "Hello, " and "world!\n", little-endian in base 256.
fn greeting_head() -> int {
    return 9056056326776136;
}

fn greeting_tail() -> int {
    return 2851464966991735;
}

fn main() -> int {
    let written = put_word(greeting_head()) + put_word(greeting_tail());
    if written == 14 {
        return 0;
    } else {
        // Unreachable unless codegen is wrong, and then the exit status says so.
        return 1;
    }
}
