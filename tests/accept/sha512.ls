// `docs/sha512.md` §4: four vectors, computed with the system
// `sha512sum` rather than from memory. The third is 112 bytes of `a` —
// the smallest input that forces the two-block path (`112 + 17 = 129 >
// 128`), the SHA-512 analogue of `sha256.ls`'s 56-byte vector.
//~ STDOUT cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e
//~ STDOUT ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f
//~ STDOUT c01d080efd492776a1c43bd23dd99d0a2e626d481e16782e75d54c2503b5dc32bd05f0f1ba33e568b88fd2d970929b719ecbb152f58f130a407c8830604b70ca
//~ STDOUT 07e547d9586f6a73f73fbac0435ed76951218fb7d0c8d788a309d785436bbb642e93a252a954f23912547d1e8a3b5ed6e1bfd7097821233fa0538f3db854fee6
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
        let digest = alloc_slice[d](64, byte_of(0));
        crypto.sha512(text, digest);
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
        check(i, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        check(i, "The quick brown fox jumps over the lazy dog");
    }

    release(io);
    return 0;
}
