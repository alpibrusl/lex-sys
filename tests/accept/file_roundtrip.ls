// `docs/filesystem.md` §3: two whole-file operations, and what they mean
// is that the bytes that went in come back out.
//
// Nothing here is file machinery beyond the two calls. The buffer is an
// `alloc_slice` from §6's arena, the comparison is the slice indexing M3
// already had, and the capability is threaded the way every other
// capability is. That is the claim the design makes -- the filesystem is
// an authority and a pair of operations, not a subsystem.
//~ STDOUT wrote 11
//~ STDOUT read 11
//~ STDOUT same
//~ EXIT 0

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

// Two slices are equal when they have the same length and the same bytes.
// `==` on `byte` is comparing storage, not arithmetic, which is why it is
// allowed where `+` is not (`docs/strings.md` §2).
fn same[&a, &b](left: &a [byte], right: &b [byte], count: int) -> [] bool {
    if len(left) < count || len(right) < count {
        return false;
    }
    var n = 0;
    while n < count {
        if left[n] != right[n] {
            return false;
        }
        n = n + 1;
    }
    return true;
}

// The row names the directory. A caller reads `fs_read("/tmp")` and
// `fs_write("/tmp")` and knows what this function can touch without
// opening it.
fn roundtrip[&f, &i](
    fs: &f Fs("/tmp"),
    io: &!i Io,
) -> [fs_read("/tmp"), fs_write("/tmp"), io] int {
    let contents = "round trip\n";
    let wrote = fs_write(fs, "/tmp/lex-sys-roundtrip.txt", contents);
    write_all(io, "wrote ");
    print_nat(io, wrote);
    putchar(io, 10);

    var status = 1;
    region a {
        // Larger than the file, so the read is the thing that decides how
        // many bytes there are rather than the buffer.
        let buffer = alloc_slice[a](64, byte_of(0));
        let read = fs_read(fs, "/tmp/lex-sys-roundtrip.txt", buffer);
        write_all(io, "read ");
        print_nat(io, read);
        putchar(io, 10);

        if read == wrote && same(contents, buffer, read) {
            write_all(io, "same\n");
            status = 0;
        }
    }
    return status;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap } = split(world);
    // This program allocates nothing on the heap, so that authority ends here.
    release(heap);
    // No foreign calls: the file operations reach libc from the backend, so
    // a program that touches files does not need the FFI capability (§2).
    release(ffi);

    let tmp = narrow(fs, "/tmp");
    var status = 1;
    borrow tmp as &f in {
        borrow mut io as &!i in {
            status = roundtrip(f, i);
        }
    }
    release(tmp);
    release(io);
    return status;
}
