// `docs/crypto.md` §5: four vectors, computed with the system `sha256sum`
// rather than from memory, and the third is the smallest input that
// forces the multi-block path (`56 + 9 = 65 > 64`).
//~ STDOUT e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
//~ STDOUT ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
//~ STDOUT 248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1
//~ STDOUT d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592
//~ EXIT 0

import std.io;
import std.crypto;

fn print_hex[&i, &d](io: &!i Io, digest: &d [byte]) -> [io_write] int {
    let alphabet = "0123456789abcdef";
    var n = 0;
    while n < len(digest) {
        let b = int_of(digest[n]);
        putchar(io, int_of(alphabet[(b >> 4) & 0xf]));
        putchar(io, int_of(alphabet[b & 0xf]));
        n = n + 1;
    }
    io.newline(io);
    return 0;
}

fn check[&i, &t](io: &!i Io, text: &t [byte]) -> [io_write] int {
    region d {
        let digest = alloc_slice[d](32, byte_of(0));
        crypto.sha256(text, digest);
        print_hex(io, digest);
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(heap);
    release(fs);
    release(ffi);

    borrow mut io as &!i in {
        check(i, "");
        check(i, "abc");
        check(i, "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        check(i, "The quick brown fox jumps over the lazy dog");
    }

    release(io);
    return 0;
}
