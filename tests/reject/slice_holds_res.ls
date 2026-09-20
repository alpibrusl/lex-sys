//~ ERROR an arena holds `val` data only

// §6.1, and a slice makes the reason sharper rather than different: the
// fill is copied into every element, and a linear value cannot be copied at
// all -- once, let alone `count` times.
//
// The rule is the one rule. An arena reclaims memory and runs nothing, so
// an obligation put inside would be dropped rather than discharged, and a
// slice would drop `count` of them.

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
    region a {
        let handles = alloc_slice[a](4, open(3));
    }
    return 0;
}
