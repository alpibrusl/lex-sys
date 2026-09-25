module std.crypto;

// `std.crypto` — SHA-256, and only SHA-256 (`docs/crypto.md`).
//
// FIPS 180-4, in 32-bit modular arithmetic expressed as masked 64-bit
// `int` (`docs/crypto.md` §2): every word this code touches lives in
// `[0, 0xffffffff]`, `mask32` reduces `mod 2^32` at the point the spec
// says to, and the checked `+` underneath never traps because the
// largest sum this code ever forms — five 32-bit terms — is nowhere
// near the 64-bit ceiling.

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
