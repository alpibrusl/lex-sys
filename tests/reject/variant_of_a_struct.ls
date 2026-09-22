//~ ERROR is a struct, not an enum
//~ RULE not-an-enum

// A variant is constructed from an enum. `Point::Origin` reads as one
// and is not: `Point` is a struct, and the refusal names the difference
// rather than reporting a missing variant, which would send a reader
// looking for a declaration to add.

struct Point { x: int, y: int }

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    let p = Point::Origin;
    return 0;
}
