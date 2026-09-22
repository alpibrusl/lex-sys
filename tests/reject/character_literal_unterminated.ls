//~ ERROR unterminated character literal
//~ RULE literal-form

// `docs/character-literals.md` §3. The mirror of
// `string_literal_spans_lines.ls`: the file ended in the middle of a
// value, and there is nothing to say about it beyond that.

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);
    release(io);
    let open = 'a;
    return 0;
}
