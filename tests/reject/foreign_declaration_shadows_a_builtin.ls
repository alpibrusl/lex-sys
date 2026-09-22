//~ ERROR cannot be declared foreign
//~ RULE foreign-declaration

// `docs/reach.md` §3: a foreign declaration names a C function, and the
// names the language already provides are not C functions. Letting one
// be redeclared as foreign would make `len` mean the builtin in one
// file and a libc symbol in another, with nothing in either signature
// saying which.

extern fn len[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    return 0;
}
