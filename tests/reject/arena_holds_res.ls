//~ ERROR an arena holds `val` data only
//~ RULE mode-bound-violated

// §6.1, and the rule that keeps an arena an *allocator* rather than a
// lifetime. Releasing one reclaims memory: it does not close files, release
// capabilities or run anything. A `res` value placed inside would have its
// memory reclaimed at block exit with its linear obligation undischarged --
// a leak with a static blessing, which is exactly the case §4 refuses to let
// affine typing paper over.
//
// The alternative -- arena teardown as a consumption event with per-object
// finalisers -- reintroduces implicit destructors and turns an O(1) release
// into a traversal. Both are things this design gave up on purpose.

res struct File {
    fd: int,
}

fn open(n: int) -> [] File {
    return File { fd: n };
}

fn close(f: File) -> [] int {
    let File { fd } = f;
    return fd;
}

fn main() -> [] int {
    let f = open(3);
    region a {
        let p = alloc[a](f);
    }
    return 0;
}
