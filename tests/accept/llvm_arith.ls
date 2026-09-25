// The LLVM backend's checked arithmetic (`docs/llvm-backend.md` §5, second
// slice): every trapping `BinOp` (`Add`, `Sub`, `Mul`, `Div`, `Rem`, `Shl`,
// `Shr`) plus the non-trapping bitwise operators, each used exactly once,
// on values chosen so the result is a printable character -- correctness
// is "did the right bytes come out", not "did it merely compile".
//
// Comparisons (`Eq`/`Ne`/`Lt`/`Le`/`Gt`/`Ge`) are implemented in the LLVM
// backend too, but this language has no way to observe a `bool` outside
// `if`/`match`, and the LLVM backend does not lower those yet -- so they
// are not in this fixture. `crates/lex-sys-codegen-llvm`'s own test suite
// notes the same gap.
//~ STDOUT Hi! OK$iK
//~ EXIT 0

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        putchar(i, 40 + 32);       // Add: 72 'H'
        putchar(i, 110 - 5);       // Sub: 105 'i'
        putchar(i, 11 * 3);        // Mul: 33 '!'
        putchar(i, 1 << 5);        // Shl: 32 ' '
        putchar(i, 64 | 15);       // BitOr: 79 'O'
        putchar(i, 79 & 75);       // BitAnd: 75 'K'
        putchar(i, 107 ^ 79);      // BitXor: 36 '$'
        putchar(i, 210 / 2);       // Div: 105 'i'
        putchar(i, 300 >> 2);      // Shr: 75 'K'
        putchar(i, 22 % 12);       // Rem: 10 '\n'
    }
    release(io);
    return 0;
}
