module std.crypto;

// `std.crypto` — SHA-256 and SHA-512 (`docs/crypto.md`, `docs/sha512.md`).
//
// FIPS 180-4. SHA-256 is 32-bit modular arithmetic expressed as masked
// 64-bit `int` (`docs/crypto.md` §2): every word this code touches lives
// in `[0, 0xffffffff]`, `mask32` reduces `mod 2^32` at the point the spec
// says to, and the checked `+` underneath never traps because the
// largest sum this code ever forms — five 32-bit terms — is nowhere
// near the 64-bit ceiling. SHA-512 needs no such mask: its word *is*
// this language's `int` width, and `wrapping_add` does the `mod 2^64`
// reduction directly — but that width match is what exposes a real gap,
// a logical right shift, that SHA-256's narrower word never could
// (`docs/sha512.md` §2).

static sha256_h0: [int] {
    let h = alloc_slice[static](8, 0);
    h[0] = 0x6a09e667;
    h[1] = 0xbb67ae85;
    h[2] = 0x3c6ef372;
    h[3] = 0xa54ff53a;
    h[4] = 0x510e527f;
    h[5] = 0x9b05688c;
    h[6] = 0x1f83d9ab;
    h[7] = 0x5be0cd19;
    return h;
}

static sha256_k: [int] {
    let k = alloc_slice[static](64, 0);
    k[0] = 0x428a2f98;
    k[1] = 0x71374491;
    k[2] = 0xb5c0fbcf;
    k[3] = 0xe9b5dba5;
    k[4] = 0x3956c25b;
    k[5] = 0x59f111f1;
    k[6] = 0x923f82a4;
    k[7] = 0xab1c5ed5;
    k[8] = 0xd807aa98;
    k[9] = 0x12835b01;
    k[10] = 0x243185be;
    k[11] = 0x550c7dc3;
    k[12] = 0x72be5d74;
    k[13] = 0x80deb1fe;
    k[14] = 0x9bdc06a7;
    k[15] = 0xc19bf174;
    k[16] = 0xe49b69c1;
    k[17] = 0xefbe4786;
    k[18] = 0x0fc19dc6;
    k[19] = 0x240ca1cc;
    k[20] = 0x2de92c6f;
    k[21] = 0x4a7484aa;
    k[22] = 0x5cb0a9dc;
    k[23] = 0x76f988da;
    k[24] = 0x983e5152;
    k[25] = 0xa831c66d;
    k[26] = 0xb00327c8;
    k[27] = 0xbf597fc7;
    k[28] = 0xc6e00bf3;
    k[29] = 0xd5a79147;
    k[30] = 0x06ca6351;
    k[31] = 0x14292967;
    k[32] = 0x27b70a85;
    k[33] = 0x2e1b2138;
    k[34] = 0x4d2c6dfc;
    k[35] = 0x53380d13;
    k[36] = 0x650a7354;
    k[37] = 0x766a0abb;
    k[38] = 0x81c2c92e;
    k[39] = 0x92722c85;
    k[40] = 0xa2bfe8a1;
    k[41] = 0xa81a664b;
    k[42] = 0xc24b8b70;
    k[43] = 0xc76c51a3;
    k[44] = 0xd192e819;
    k[45] = 0xd6990624;
    k[46] = 0xf40e3585;
    k[47] = 0x106aa070;
    k[48] = 0x19a4c116;
    k[49] = 0x1e376c08;
    k[50] = 0x2748774c;
    k[51] = 0x34b0bcb5;
    k[52] = 0x391c0cb3;
    k[53] = 0x4ed8aa4a;
    k[54] = 0x5b9cca4f;
    k[55] = 0x682e6ff3;
    k[56] = 0x748f82ee;
    k[57] = 0x78a5636f;
    k[58] = 0x84c87814;
    k[59] = 0x8cc70208;
    k[60] = 0x90befffa;
    k[61] = 0xa4506ceb;
    k[62] = 0xbef9a3f7;
    k[63] = 0xc67178f2;
    return k;
}

fn mask32(x: int) -> [] int {
    return x & 0xffffffff;
}

// A 32-bit rotate built out of two opposite shifts and a masked `|` —
// this language has no dedicated rotate operator (`docs/crypto.md` §2).
fn rotr32(x: int, n: int) -> [] int {
    return mask32((x >> n) | (x << (32 - n)));
}

// The 32-bit bitwise complement of a value already confined to
// `[0, 0xffffffff]`, built out of subtraction rather than `~` (which
// flips all 64 bits, not 32) — a true identity on that range, not an
// approximation (`docs/crypto.md` §2).
fn not32(x: int) -> [] int {
    return 0xffffffff - x;
}

// One 64-byte block, folded into `state` — eight running 32-bit words,
// held as `int`s in `[0, 0xffffffff]`.
fn compress[&st, &b](state: &!st [int], block: &b [byte]) -> [] int {
    region a {
        let w = alloc_slice[a](64, 0);

        var t = 0;
        while t < 16 {
            let i = t * 4;
            w[t] = (int_of(block[i]) << 24)
                | (int_of(block[i + 1]) << 16)
                | (int_of(block[i + 2]) << 8)
                | int_of(block[i + 3]);
            t = t + 1;
        }

        t = 16;
        while t < 64 {
            let s0 = rotr32(w[t - 15], 7) ^ rotr32(w[t - 15], 18) ^ (w[t - 15] >> 3);
            let s1 = rotr32(w[t - 2], 17) ^ rotr32(w[t - 2], 19) ^ (w[t - 2] >> 10);
            w[t] = mask32(w[t - 16] + s0 + w[t - 7] + s1);
            t = t + 1;
        }

        var wa = state[0];
        var wb = state[1];
        var wc = state[2];
        var wd = state[3];
        var we = state[4];
        var wf = state[5];
        var wg = state[6];
        var wh = state[7];

        t = 0;
        while t < 64 {
            let s1 = rotr32(we, 6) ^ rotr32(we, 11) ^ rotr32(we, 25);
            let ch = (we & wf) ^ (not32(we) & wg);
            let temp1 = mask32(wh + s1 + ch + sha256_k[t] + w[t]);
            let s0 = rotr32(wa, 2) ^ rotr32(wa, 13) ^ rotr32(wa, 22);
            let maj = (wa & wb) ^ (wa & wc) ^ (wb & wc);
            let temp2 = mask32(s0 + maj);

            wh = wg;
            wg = wf;
            wf = we;
            we = mask32(wd + temp1);
            wd = wc;
            wc = wb;
            wb = wa;
            wa = mask32(temp1 + temp2);

            t = t + 1;
        }

        state[0] = mask32(state[0] + wa);
        state[1] = mask32(state[1] + wb);
        state[2] = mask32(state[2] + wc);
        state[3] = mask32(state[3] + wd);
        state[4] = mask32(state[4] + we);
        state[5] = mask32(state[5] + wf);
        state[6] = mask32(state[6] + wg);
        state[7] = mask32(state[7] + wh);
    }
    return 0;
}

// `message`, hashed into `digest` — a caller-provided, unique-referenced
// output buffer of at least 32 bytes, the same "write into what the
// caller passed" shape `std.bignum`'s `add_into`/`copy` already use
// (`docs/crypto.md` §4). A shorter `digest` traps on the ordinary bounds
// check every other out-of-range write in this language already gets.
pub fn sha256[&s, &o](message: &s [byte], digest: &!o [byte]) -> [] int {
    region st {
        let state = alloc_slice[st](8, 0);
        var i = 0;
        while i < 8 {
            state[i] = sha256_h0[i];
            i = i + 1;
        }

        let total = len(message);
        // 1 byte for 0x80, 8 for the big-endian bit length, rounded up
        // to a multiple of 64 (FIPS 180-4 §5.1.1).
        let padded_len = ((total + 9 + 63) / 64) * 64;

        region m {
            let padded = alloc_slice[m](padded_len, byte_of(0));
            var j = 0;
            while j < total {
                padded[j] = message[j];
                j = j + 1;
            }
            padded[total] = byte_of(0x80);
            // The zero bytes between the 0x80 marker and the length
            // field are already there: `alloc_slice`'s own fill value.

            let bit_len = total * 8;
            var k = 0;
            while k < 8 {
                let shift = (7 - k) * 8;
                padded[padded_len - 8 + k] = byte_of((bit_len >> shift) & 0xff);
                k = k + 1;
            }

            var block = 0;
            while block < padded_len {
                compress(state, padded[block..block + 64]);
                block = block + 64;
            }
        }

        var w = 0;
        while w < 8 {
            let word = state[w];
            digest[w * 4] = byte_of((word >> 24) & 0xff);
            digest[w * 4 + 1] = byte_of((word >> 16) & 0xff);
            digest[w * 4 + 2] = byte_of((word >> 8) & 0xff);
            digest[w * 4 + 3] = byte_of(word & 0xff);
            w = w + 1;
        }
    }
    return 0;
}

// SHA-512 (`docs/sha512.md`). Unlike SHA-256's words, a SHA-512 word
// *is* this language's `int` width, so there is no room to mask a
// result into a narrower range the way `mask32` does — the reduction
// `mod 2^64` is exactly what `wrapping_add` already computes, bit for
// bit, on the full 64 bits.

static sha512_h0: [int] {
    let h = alloc_slice[static](8, 0);
    h[0] = (0x6a09e667 << 32) | 0xf3bcc908;
    h[1] = (0xbb67ae85 << 32) | 0x84caa73b;
    h[2] = (0x3c6ef372 << 32) | 0xfe94f82b;
    h[3] = (0xa54ff53a << 32) | 0x5f1d36f1;
    h[4] = (0x510e527f << 32) | 0xade682d1;
    h[5] = (0x9b05688c << 32) | 0x2b3e6c1f;
    h[6] = (0x1f83d9ab << 32) | 0xfb41bd6b;
    h[7] = (0x5be0cd19 << 32) | 0x137e2179;
    return h;
}

static sha512_k: [int] {
    let k = alloc_slice[static](80, 0);
    k[0] = (0x428a2f98 << 32) | 0xd728ae22;
    k[1] = (0x71374491 << 32) | 0x23ef65cd;
    k[2] = (0xb5c0fbcf << 32) | 0xec4d3b2f;
    k[3] = (0xe9b5dba5 << 32) | 0x8189dbbc;
    k[4] = (0x3956c25b << 32) | 0xf348b538;
    k[5] = (0x59f111f1 << 32) | 0xb605d019;
    k[6] = (0x923f82a4 << 32) | 0xaf194f9b;
    k[7] = (0xab1c5ed5 << 32) | 0xda6d8118;
    k[8] = (0xd807aa98 << 32) | 0xa3030242;
    k[9] = (0x12835b01 << 32) | 0x45706fbe;
    k[10] = (0x243185be << 32) | 0x4ee4b28c;
    k[11] = (0x550c7dc3 << 32) | 0xd5ffb4e2;
    k[12] = (0x72be5d74 << 32) | 0xf27b896f;
    k[13] = (0x80deb1fe << 32) | 0x3b1696b1;
    k[14] = (0x9bdc06a7 << 32) | 0x25c71235;
    k[15] = (0xc19bf174 << 32) | 0xcf692694;
    k[16] = (0xe49b69c1 << 32) | 0x9ef14ad2;
    k[17] = (0xefbe4786 << 32) | 0x384f25e3;
    k[18] = (0x0fc19dc6 << 32) | 0x8b8cd5b5;
    k[19] = (0x240ca1cc << 32) | 0x77ac9c65;
    k[20] = (0x2de92c6f << 32) | 0x592b0275;
    k[21] = (0x4a7484aa << 32) | 0x6ea6e483;
    k[22] = (0x5cb0a9dc << 32) | 0xbd41fbd4;
    k[23] = (0x76f988da << 32) | 0x831153b5;
    k[24] = (0x983e5152 << 32) | 0xee66dfab;
    k[25] = (0xa831c66d << 32) | 0x2db43210;
    k[26] = (0xb00327c8 << 32) | 0x98fb213f;
    k[27] = (0xbf597fc7 << 32) | 0xbeef0ee4;
    k[28] = (0xc6e00bf3 << 32) | 0x3da88fc2;
    k[29] = (0xd5a79147 << 32) | 0x930aa725;
    k[30] = (0x06ca6351 << 32) | 0xe003826f;
    k[31] = (0x14292967 << 32) | 0x0a0e6e70;
    k[32] = (0x27b70a85 << 32) | 0x46d22ffc;
    k[33] = (0x2e1b2138 << 32) | 0x5c26c926;
    k[34] = (0x4d2c6dfc << 32) | 0x5ac42aed;
    k[35] = (0x53380d13 << 32) | 0x9d95b3df;
    k[36] = (0x650a7354 << 32) | 0x8baf63de;
    k[37] = (0x766a0abb << 32) | 0x3c77b2a8;
    k[38] = (0x81c2c92e << 32) | 0x47edaee6;
    k[39] = (0x92722c85 << 32) | 0x1482353b;
    k[40] = (0xa2bfe8a1 << 32) | 0x4cf10364;
    k[41] = (0xa81a664b << 32) | 0xbc423001;
    k[42] = (0xc24b8b70 << 32) | 0xd0f89791;
    k[43] = (0xc76c51a3 << 32) | 0x0654be30;
    k[44] = (0xd192e819 << 32) | 0xd6ef5218;
    k[45] = (0xd6990624 << 32) | 0x5565a910;
    k[46] = (0xf40e3585 << 32) | 0x5771202a;
    k[47] = (0x106aa070 << 32) | 0x32bbd1b8;
    k[48] = (0x19a4c116 << 32) | 0xb8d2d0c8;
    k[49] = (0x1e376c08 << 32) | 0x5141ab53;
    k[50] = (0x2748774c << 32) | 0xdf8eeb99;
    k[51] = (0x34b0bcb5 << 32) | 0xe19b48a8;
    k[52] = (0x391c0cb3 << 32) | 0xc5c95a63;
    k[53] = (0x4ed8aa4a << 32) | 0xe3418acb;
    k[54] = (0x5b9cca4f << 32) | 0x7763e373;
    k[55] = (0x682e6ff3 << 32) | 0xd6b2b8a3;
    k[56] = (0x748f82ee << 32) | 0x5defb2fc;
    k[57] = (0x78a5636f << 32) | 0x43172f60;
    k[58] = (0x84c87814 << 32) | 0xa1f0ab72;
    k[59] = (0x8cc70208 << 32) | 0x1a6439ec;
    k[60] = (0x90befffa << 32) | 0x23631e28;
    k[61] = (0xa4506ceb << 32) | 0xde82bde9;
    k[62] = (0xbef9a3f7 << 32) | 0xb2c67915;
    k[63] = (0xc67178f2 << 32) | 0xe372532b;
    k[64] = (0xca273ece << 32) | 0xea26619c;
    k[65] = (0xd186b8c7 << 32) | 0x21c0c207;
    k[66] = (0xeada7dd6 << 32) | 0xcde0eb1e;
    k[67] = (0xf57d4f7f << 32) | 0xee6ed178;
    k[68] = (0x06f067aa << 32) | 0x72176fba;
    k[69] = (0x0a637dc5 << 32) | 0xa2c898a6;
    k[70] = (0x113f9804 << 32) | 0xbef90dae;
    k[71] = (0x1b710b35 << 32) | 0x131c471b;
    k[72] = (0x28db77f5 << 32) | 0x23047d84;
    k[73] = (0x32caab7b << 32) | 0x40c72493;
    k[74] = (0x3c9ebe0a << 32) | 0x15c9bebc;
    k[75] = (0x431d67c4 << 32) | 0x9c100d4c;
    k[76] = (0x4cc5d4be << 32) | 0xcb3e42b6;
    k[77] = (0x597f299c << 32) | 0xfc657e2a;
    k[78] = (0x5fcb6fab << 32) | 0x3ad6faec;
    k[79] = (0x6c44198c << 32) | 0x4a475817;
    return k;
}

// `2^k - 1`, the low-`k`-bits mask a logical shift needs. Not `(1 <<
// k) - 1` for every `k`: at `k = 63` that expression is `int::MIN - 1`,
// which traps (`docs/sha512.md` §2) — the one `k` this file's own call
// sites reach, from `rotr64(x, 1)`. Written as the literal instead,
// which is what the value always was.
fn low_mask64(k: int) -> [] int {
    if k == 63 {
        return 0x7fffffffffffffff;
    }
    return (1 << k) - 1;
}

// The logical right shift this language does not have
// (`docs/bitwise.md` §2, `docs/sha512.md` §2): `>>` sign-extends, so a
// plain `x >> n` is wrong whenever `x`'s top bit is set and the result
// still has to be used as bits rather than discarded by a narrower
// mask. `(x >> n)` already has the correct low `64 - n` bits regardless
// of sign; masking off the top `n` — which arithmetic shift may have
// filled with ones — is the fix.
fn lshr64(x: int, n: int) -> [] int {
    return (x >> n) & low_mask64(64 - n);
}

// A 64-bit rotate built the same way `rotr32` builds a 32-bit one, on
// the logical shift above rather than `>>` directly: the two halves
// land in disjoint bit ranges (`lshr64` zero-fills the top `n`; `x <<
// (64 - n)` zero-fills the bottom `64 - n`, `docs/bitwise.md` §4), so
// no mask is needed on the `|` itself.
fn rotr64(x: int, n: int) -> [] int {
    return lshr64(x, n) | (x << (64 - n));
}

// One 128-byte block, folded into `state` — eight running 64-bit words.
// Structurally `compress` again, at double the word width and eighty
// rounds instead of sixty-four; the differences are the schedule and
// round constants (`docs/sha512.md` §1) and that every addition is
// `wrapping_add` rather than a checked `+` under a mask, since there is
// no narrower width to mask into (`docs/sha512.md` §2). `~e`, not a
// `not64` mirroring `not32`, is the true 64-bit complement here — the
// SHA-256 file needed `not32` only because `~` flips more bits than its
// masked word has, and a full-width word has no such gap.
fn compress512[&st, &b](state: &!st [int], block: &b [byte]) -> [] int {
    region a {
        let w = alloc_slice[a](80, 0);

        var t = 0;
        while t < 16 {
            let i = t * 8;
            w[t] = (int_of(block[i]) << 56)
                | (int_of(block[i + 1]) << 48)
                | (int_of(block[i + 2]) << 40)
                | (int_of(block[i + 3]) << 32)
                | (int_of(block[i + 4]) << 24)
                | (int_of(block[i + 5]) << 16)
                | (int_of(block[i + 6]) << 8)
                | int_of(block[i + 7]);
            t = t + 1;
        }

        t = 16;
        while t < 80 {
            let s0 = rotr64(w[t - 15], 1) ^ rotr64(w[t - 15], 8) ^ lshr64(w[t - 15], 7);
            let s1 = rotr64(w[t - 2], 19) ^ rotr64(w[t - 2], 61) ^ lshr64(w[t - 2], 6);
            w[t] = wrapping_add(wrapping_add(w[t - 16], s0), wrapping_add(w[t - 7], s1));
            t = t + 1;
        }

        var wa = state[0];
        var wb = state[1];
        var wc = state[2];
        var wd = state[3];
        var we = state[4];
        var wf = state[5];
        var wg = state[6];
        var wh = state[7];

        t = 0;
        while t < 80 {
            let s1 = rotr64(we, 14) ^ rotr64(we, 18) ^ rotr64(we, 41);
            let ch = (we & wf) ^ (~we & wg);
            let temp1 = wrapping_add(wrapping_add(wrapping_add(wh, s1), ch), wrapping_add(sha512_k[t], w[t]));
            let s0 = rotr64(wa, 28) ^ rotr64(wa, 34) ^ rotr64(wa, 39);
            let maj = (wa & wb) ^ (wa & wc) ^ (wb & wc);
            let temp2 = wrapping_add(s0, maj);

            wh = wg;
            wg = wf;
            wf = we;
            we = wrapping_add(wd, temp1);
            wd = wc;
            wc = wb;
            wb = wa;
            wa = wrapping_add(temp1, temp2);

            t = t + 1;
        }

        state[0] = wrapping_add(state[0], wa);
        state[1] = wrapping_add(state[1], wb);
        state[2] = wrapping_add(state[2], wc);
        state[3] = wrapping_add(state[3], wd);
        state[4] = wrapping_add(state[4], we);
        state[5] = wrapping_add(state[5], wf);
        state[6] = wrapping_add(state[6], wg);
        state[7] = wrapping_add(state[7], wh);
    }
    return 0;
}

// `message`, hashed into `digest` — a caller-provided, unique-referenced
// output buffer of at least 64 bytes, the same shape `sha256` above
// already uses. The 16-byte big-endian bit-length field (FIPS 180-4
// §5.1.2) is wider than this language's `int`: only the low 8 bytes are
// ever written, the top 8 stay at `alloc_slice`'s own zero fill, and
// that is exact rather than truncated for every message this language
// can even hold — `total * 8` is a checked multiply, so a message long
// enough to need the other 8 bytes traps building the length field
// rather than silently wrapping past it (`docs/sha512.md` §3).
pub fn sha512[&s, &o](message: &s [byte], digest: &!o [byte]) -> [] int {
    region st {
        let state = alloc_slice[st](8, 0);
        var i = 0;
        while i < 8 {
            state[i] = sha512_h0[i];
            i = i + 1;
        }

        let total = len(message);
        // 1 byte for 0x80, 16 for the big-endian bit length, rounded up
        // to a multiple of 128 (FIPS 180-4 §5.1.2).
        let padded_len = ((total + 17 + 127) / 128) * 128;

        region m {
            let padded = alloc_slice[m](padded_len, byte_of(0));
            var j = 0;
            while j < total {
                padded[j] = message[j];
                j = j + 1;
            }
            padded[total] = byte_of(0x80);
            // The zero bytes between the 0x80 marker and the length
            // field are already there: `alloc_slice`'s own fill value.
            // So are the length field's own top 8 bytes — see above.

            let bit_len = total * 8;
            var k = 0;
            while k < 8 {
                let shift = (7 - k) * 8;
                padded[padded_len - 8 + k] = byte_of((bit_len >> shift) & 0xff);
                k = k + 1;
            }

            var block = 0;
            while block < padded_len {
                compress512(state, padded[block..block + 128]);
                block = block + 128;
            }
        }

        var w = 0;
        while w < 8 {
            let word = state[w];
            digest[w * 8] = byte_of((word >> 56) & 0xff);
            digest[w * 8 + 1] = byte_of((word >> 48) & 0xff);
            digest[w * 8 + 2] = byte_of((word >> 40) & 0xff);
            digest[w * 8 + 3] = byte_of((word >> 32) & 0xff);
            digest[w * 8 + 4] = byte_of((word >> 24) & 0xff);
            digest[w * 8 + 5] = byte_of((word >> 16) & 0xff);
            digest[w * 8 + 6] = byte_of((word >> 8) & 0xff);
            digest[w * 8 + 7] = byte_of(word & 0xff);
            w = w + 1;
        }
    }
    return 0;
}
