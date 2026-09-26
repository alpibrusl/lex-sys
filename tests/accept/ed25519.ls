// `docs/ed25519.md` §5: three vectors, each a real Ed25519 keypair
// generated and signed by the system `openssl`, not from memory --
// `openssl genpkey -algorithm ed25519`, `openssl pkeyutl -sign -rawin`,
// cross-checked with `openssl pkeyutl -verify` before being used here.
//~ STDOUT 228ada2141ab7425651a4ce8d9c5ed0f720319957a1035218354a81f950b480ee5ad13cad8eec58674f044f1e87e3795c91c303e2664272ab2c9b2595a67ee02
//~ STDOUT 1
//~ STDOUT 0
//~ STDOUT 58cfcc803d68d21df7d5ecbc10a5dce09e549da84ef7dc664240799091ad2a5fca20f458498d91832e4012f339a4a0cb28934209b36b401a46d722ac9449db04
//~ STDOUT 1
//~ STDOUT 0
//~ STDOUT 8760c2941c58fb6b2cdd57b3b7ede001de1e9bcd1955bf243a8a0a72a7363ec77fe0e0b1ae1b5b045506bb61f943c2791756ff14fefc8925db194069df30bb07
//~ STDOUT 1
//~ STDOUT 0
//~ EXIT 0

import std.io;
import std.ed25519;

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

fn nibble(c: int) -> [] int {
    if c >= 48 {
        if c <= 57 {
            return c - 48;
        }
    }
    return c - 97 + 10;
}

fn hex_decode[&t, &o](text: &t [byte], o: &!o [byte]) -> [] int {
    var i = 0;
    let n = len(o);
    while i < n {
        let hi = nibble(int_of(text[i * 2]));
        let lo = nibble(int_of(text[i * 2 + 1]));
        o[i] = byte_of(hi * 16 + lo);
        i = i + 1;
    }
    return 0;
}

// Signs `msg` with `seed_hex`, prints the signature, prints `1` for a
// genuine verify and `0` once the signature is tampered with.
fn check_one[&i, &seed_hex, &msg](
    io: &!i Io,
    seed_hex: &seed_hex [byte],
    msg: &msg [byte],
) -> [io_write] int {
    region r {
        let seed = alloc_slice[r](32, byte_of(0));
        hex_decode(seed_hex, seed);
        let sig = alloc_slice[r](64, byte_of(0));
        ed25519.sign(seed, msg, sig);
        print_hex(io, sig);

        let pub_ = alloc_slice[r](32, byte_of(0));
        ed25519.public_key_from_seed(seed, pub_);
        let ok = ed25519.verify(pub_, msg, sig);
        putchar(io, 48 + ok);
        io.newline(io);

        sig[0] = byte_of(int_of(sig[0]) ^ 1);
        let bad = ed25519.verify(pub_, msg, sig);
        putchar(io, 48 + bad);
        io.newline(io);
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
        check_one(i, "413e712d11b9fcb4af989036dd2f253106e2f0d49a1d954b1880a98308b698af", "a");
        check_one(
            i,
            "b22b6ce8505d2603e5afabda2cc0b3844fdd6e5a7a74cf81ebd78c9d2ad95e80",
            "hello world",
        );
        check_one(
            i,
            "a5541feee56a353b0570e1dbd90d46232897f5379e68392402c2b619ca9e50ea",
            "The quick brown fox jumps over the lazy dog",
        );
    }

    release(io);
    return 0;
}
