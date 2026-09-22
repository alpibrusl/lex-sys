//~ ERROR is not a region parameter of
//~ RULE region-mismatch

// A `where` clause relates the regions a signature already takes. One
// naming a region the signature does not declare relates nothing, and
// reads as a constraint being enforced when there is none.

fn pick[&a where b <= a](first: &a int) -> [] &a int {
    return first;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    return 0;
}
