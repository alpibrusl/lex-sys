//~ ERROR may not outlive its region

// A buffer built in an arena is a slice into that arena, and §6 of
// `linearity-and-effects.md` says nothing whose type mentions the region
// leaves the block. A string is a slice, so it is the same rule, run by the
// same code -- there is no separate escape analysis for strings.
//
// A *literal* may be returned, because `static` outlives everything:
// `tests/accept/string_literal.ls` does exactly that. The difference is
// where the bytes are, which is what the region records.

fn build() -> [] &static [byte] {
    region a {
        let buffer = alloc_slice[a](4, byte_of(65));
        return buffer;
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program touches no files, so that authority ends here.
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    return 0;
}
