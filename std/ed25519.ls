module std.ed25519;

// Ed25519 (RFC 8032). `docs/ed25519.md` is the design; §6 is explicit
// about what this module does not provide: constant-time execution.
// Every operation here is ordinary conditional branching and ordinary
// arithmetic, so an attacker with a timing oracle on repeated signing
// operations could learn about the secret scalar. Correct on every
// checked vector is not the same claim as safe against that attacker.
// §1 is also explicit about who this is for: a future lex-sys port of
// lex-os-guest, not this repository's own Rust tooling, which already
// has `ed25519-dalek` and should keep using it.

// A "big number" here is a little-endian [byte] slice, most often 32
// bytes (256 bits) -- wide enough to hold the field modulus (255 bits)
// and the group order (253 bits) with room to spare, and to let every
// intermediate value in a reduction stay inside one fixed width rather
// than growing without bound. Correctness first: reduction is bit-serial
// long division against a fixed modulus, not the carry-chain tricks a
// fast field-arithmetic library would use -- more instructions per
// operation, far fewer places to get the arithmetic wrong.

fn bn_compare[&a, &b](a: &a [byte], b: &b [byte]) -> [] int {
    var i = len(a) - 1;
    while i >= 0 {
        let av = int_of(a[i]);
        let bv = int_of(b[i]);
        if av < bv {
            return -1;
        }
        if av > bv {
            return 1;
        }
        i = i - 1;
    }
    return 0;
}

fn bn_copy_into[&d, &a](d: &!d [byte], a: &a [byte]) -> [] int {
    var i = 0;
    let n = len(a);
    while i < n {
        d[i] = a[i];
        i = i + 1;
    }
    return 0;
}

// `a += b`, in place, equal length. Returns the carry out of the top
// byte; every call site in this module sizes its buffers so that carry
// is always 0 (a + b never needs a wider representation), and relies on
// the checked bounds of the buffer itself to catch it otherwise.
fn bn_add_into[&a, &b](a: &!a [byte], b: &b [byte]) -> [] int {
    var carry = 0;
    var i = 0;
    let n = len(a);
    while i < n {
        let sum = int_of(a[i]) + int_of(b[i]) + carry;
        a[i] = byte_of(sum & 0xff);
        carry = sum >> 8;
        i = i + 1;
    }
    return carry;
}

// `a -= b`, in place, equal length, assuming `a >= b`. Every call site
// checks that with `bn_compare` first (or arranges it structurally) --
// this function does not check it itself and produces a wrapped-around
// answer rather than a trap if it does not hold.
fn bn_sub_into[&a, &b](a: &!a [byte], b: &b [byte]) -> [] int {
    var need = 0;
    var i = 0;
    let n = len(a);
    while i < n {
        var diff = int_of(a[i]) - int_of(b[i]) - need;
        if diff < 0 {
            diff = diff + 256;
            need = 1;
        } else {
            need = 0;
        }
        a[i] = byte_of(diff);
        i = i + 1;
    }
    return need;
}

// `a = (a << 1) | bit_in`, within `a`'s own width. Every call site in
// this module keeps `a < m` before the call for whatever modulus it is
// reducing against, and `m` fits in 255 bits, so `a << 1` never needs a
// bit beyond the buffer's own width -- checked directly by
// `the_shift_used_by_reduction_never_overflows_its_buffer` rather than
// assumed.
fn bn_shl1_into[&a](a: &!a [byte], bit_in: int) -> [] int {
    var carry = bit_in;
    var i = 0;
    let n = len(a);
    while i < n {
        let v = int_of(a[i]);
        let new_carry = (v >> 7) & 1;
        a[i] = byte_of(((v << 1) & 0xff) | carry);
        carry = new_carry;
        i = i + 1;
    }
    return carry;
}

// `o = a * b`, schoolbook, `o` pre-zeroed and `len(o) == len(a) + len(b)`.
fn bn_mul_full[&o, &a, &b](o: &!o [byte], a: &a [byte], b: &b [byte]) -> [] int {
    let na = len(a);
    let nb = len(b);
    var i = 0;
    while i < na {
        var carry = 0;
        var j = 0;
        while j < nb {
            let prod = int_of(a[i]) * int_of(b[j]) + int_of(o[i + j]) + carry;
            o[i + j] = byte_of(prod & 0xff);
            carry = prod >> 8;
            j = j + 1;
        }
        var k = i + nb;
        while carry > 0 {
            let sum = int_of(o[k]) + carry;
            o[k] = byte_of(sum & 0xff);
            carry = sum >> 8;
            k = k + 1;
        }
        i = i + 1;
    }
    return 0;
}

// `o = x mod m`, `o` pre-zeroed and `len(o) == len(m)`. Bit-serial long
// division: shift the running remainder left by one bit of `x` at a
// time, from the most significant end, subtracting `m` back out
// whenever the remainder reaches it. The remainder never needs more
// bits than `m` has plus one, which is why `o` only ever needs to be as
// wide as `m` itself, whatever `x`'s own width is.
fn bn_reduce_wide[&o, &x, &m](o: &!o [byte], x: &x [byte], m: &m [byte]) -> [] int {
    // `o` is the running remainder, built up one bit at a time from
    // nothing -- zeroed here rather than left to the caller, because a
    // caller that reuses one scratch buffer across many calls (`bn_modpow`
    // does, once per bit of the exponent) would otherwise start each
    // reduction from whatever the previous call left behind. Found by a
    // failing `3^1 mod 7` in this module's own tests: correct in
    // isolation, wrong the second time the same buffer was reused.
    var z = 0;
    let no = len(o);
    while z < no {
        o[z] = byte_of(0);
        z = z + 1;
    }
    let nx = len(x);
    var bit = nx * 8 - 1;
    while bit >= 0 {
        let byte_idx = bit / 8;
        let bit_idx = bit % 8;
        let b = (int_of(x[byte_idx]) >> bit_idx) & 1;
        bn_shl1_into(o, b);
        if bn_compare(o, m) >= 0 {
            bn_sub_into(o, m);
        }
        bit = bit - 1;
    }
    return 0;
}

fn bn_mulmod[&o, &a, &b, &m](o: &!o [byte], a: &a [byte], b: &b [byte], m: &m [byte]) -> [] int {
    region r {
        let wide = alloc_slice[r](len(a) + len(b), byte_of(0));
        bn_mul_full(wide, a, b);
        bn_reduce_wide(o, wide, m);
    }
    return 0;
}

// `a` and `b` are both already reduced (`< m`), so `a + b < 2m`, which
// fits in the same width `m` does -- at most one conditional subtract
// brings it back under `m`.
fn bn_addmod[&o, &a, &b, &m](o: &!o [byte], a: &a [byte], b: &b [byte], m: &m [byte]) -> [] int {
    bn_copy_into(o, a);
    bn_add_into(o, b);
    if bn_compare(o, m) >= 0 {
        bn_sub_into(o, m);
    }
    return 0;
}

fn bn_submod[&o, &a, &b, &m](o: &!o [byte], a: &a [byte], b: &b [byte], m: &m [byte]) -> [] int {
    bn_copy_into(o, a);
    if bn_compare(a, b) < 0 {
        bn_add_into(o, m);
    }
    bn_sub_into(o, b);
    return 0;
}

// `o = base^exp mod m`, left-to-right square-and-multiply. `exp`'s width
// decides the loop's own length, so the same function serves both the
// field's `p - 2` inversion exponent and its `(p + 3) / 8` square-root
// exponent without a second copy.
fn bn_modpow[&o, &base, &exp, &m](
    o: &!o [byte],
    base: &base [byte],
    exp: &exp [byte],
    m: &m [byte],
) -> [] int {
    region r {
        let result = alloc_slice[r](len(m), byte_of(0));
        result[0] = byte_of(1);
        let scratch = alloc_slice[r](len(m), byte_of(0));
        var bit = len(exp) * 8 - 1;
        while bit >= 0 {
            bn_mulmod(scratch, result, result, m);
            bn_copy_into(result, scratch);
            let byte_idx = bit / 8;
            let bit_idx = bit % 8;
            let b = (int_of(exp[byte_idx]) >> bit_idx) & 1;
            if b == 1 {
                bn_mulmod(scratch, result, base, m);
                bn_copy_into(result, scratch);
            }
            bit = bit - 1;
        }
        bn_copy_into(o, result);
    }
    return 0;
}
// 2^255 - 19, the field modulus.
static p_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xed);
    k[1] = byte_of(0xff);
    k[2] = byte_of(0xff);
    k[3] = byte_of(0xff);
    k[4] = byte_of(0xff);
    k[5] = byte_of(0xff);
    k[6] = byte_of(0xff);
    k[7] = byte_of(0xff);
    k[8] = byte_of(0xff);
    k[9] = byte_of(0xff);
    k[10] = byte_of(0xff);
    k[11] = byte_of(0xff);
    k[12] = byte_of(0xff);
    k[13] = byte_of(0xff);
    k[14] = byte_of(0xff);
    k[15] = byte_of(0xff);
    k[16] = byte_of(0xff);
    k[17] = byte_of(0xff);
    k[18] = byte_of(0xff);
    k[19] = byte_of(0xff);
    k[20] = byte_of(0xff);
    k[21] = byte_of(0xff);
    k[22] = byte_of(0xff);
    k[23] = byte_of(0xff);
    k[24] = byte_of(0xff);
    k[25] = byte_of(0xff);
    k[26] = byte_of(0xff);
    k[27] = byte_of(0xff);
    k[28] = byte_of(0xff);
    k[29] = byte_of(0xff);
    k[30] = byte_of(0xff);
    k[31] = byte_of(0x7f);
    return k;
}

// the group order, 2^252 + 27742317777372353535851937790883648493.
static l_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xed);
    k[1] = byte_of(0xd3);
    k[2] = byte_of(0xf5);
    k[3] = byte_of(0x5c);
    k[4] = byte_of(0x1a);
    k[5] = byte_of(0x63);
    k[6] = byte_of(0x12);
    k[7] = byte_of(0x58);
    k[8] = byte_of(0xd6);
    k[9] = byte_of(0x9c);
    k[10] = byte_of(0xf7);
    k[11] = byte_of(0xa2);
    k[12] = byte_of(0xde);
    k[13] = byte_of(0xf9);
    k[14] = byte_of(0xde);
    k[15] = byte_of(0x14);
    k[16] = byte_of(0x00);
    k[17] = byte_of(0x00);
    k[18] = byte_of(0x00);
    k[19] = byte_of(0x00);
    k[20] = byte_of(0x00);
    k[21] = byte_of(0x00);
    k[22] = byte_of(0x00);
    k[23] = byte_of(0x00);
    k[24] = byte_of(0x00);
    k[25] = byte_of(0x00);
    k[26] = byte_of(0x00);
    k[27] = byte_of(0x00);
    k[28] = byte_of(0x00);
    k[29] = byte_of(0x00);
    k[30] = byte_of(0x00);
    k[31] = byte_of(0x10);
    return k;
}

// the twisted Edwards curve parameter, -121665/121666 mod p.
static d_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xa3);
    k[1] = byte_of(0x78);
    k[2] = byte_of(0x59);
    k[3] = byte_of(0x13);
    k[4] = byte_of(0xca);
    k[5] = byte_of(0x4d);
    k[6] = byte_of(0xeb);
    k[7] = byte_of(0x75);
    k[8] = byte_of(0xab);
    k[9] = byte_of(0xd8);
    k[10] = byte_of(0x41);
    k[11] = byte_of(0x41);
    k[12] = byte_of(0x4d);
    k[13] = byte_of(0x0a);
    k[14] = byte_of(0x70);
    k[15] = byte_of(0x00);
    k[16] = byte_of(0x98);
    k[17] = byte_of(0xe8);
    k[18] = byte_of(0x79);
    k[19] = byte_of(0x77);
    k[20] = byte_of(0x79);
    k[21] = byte_of(0x40);
    k[22] = byte_of(0xc7);
    k[23] = byte_of(0x8c);
    k[24] = byte_of(0x73);
    k[25] = byte_of(0xfe);
    k[26] = byte_of(0x6f);
    k[27] = byte_of(0x2b);
    k[28] = byte_of(0xee);
    k[29] = byte_of(0x6c);
    k[30] = byte_of(0x03);
    k[31] = byte_of(0x52);
    return k;
}

// a square root of -1 mod p, needed by point decompression.
static sqrt_m1_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xb0);
    k[1] = byte_of(0xa0);
    k[2] = byte_of(0x0e);
    k[3] = byte_of(0x4a);
    k[4] = byte_of(0x27);
    k[5] = byte_of(0x1b);
    k[6] = byte_of(0xee);
    k[7] = byte_of(0xc4);
    k[8] = byte_of(0x78);
    k[9] = byte_of(0xe4);
    k[10] = byte_of(0x2f);
    k[11] = byte_of(0xad);
    k[12] = byte_of(0x06);
    k[13] = byte_of(0x18);
    k[14] = byte_of(0x43);
    k[15] = byte_of(0x2f);
    k[16] = byte_of(0xa7);
    k[17] = byte_of(0xd7);
    k[18] = byte_of(0xfb);
    k[19] = byte_of(0x3d);
    k[20] = byte_of(0x99);
    k[21] = byte_of(0x00);
    k[22] = byte_of(0x4d);
    k[23] = byte_of(0x2b);
    k[24] = byte_of(0x0b);
    k[25] = byte_of(0xdf);
    k[26] = byte_of(0xc1);
    k[27] = byte_of(0x4f);
    k[28] = byte_of(0x80);
    k[29] = byte_of(0x24);
    k[30] = byte_of(0x83);
    k[31] = byte_of(0x2b);
    return k;
}

// p - 2, the exponent modular inversion uses (Fermat's little theorem).
static exp_inv_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xeb);
    k[1] = byte_of(0xff);
    k[2] = byte_of(0xff);
    k[3] = byte_of(0xff);
    k[4] = byte_of(0xff);
    k[5] = byte_of(0xff);
    k[6] = byte_of(0xff);
    k[7] = byte_of(0xff);
    k[8] = byte_of(0xff);
    k[9] = byte_of(0xff);
    k[10] = byte_of(0xff);
    k[11] = byte_of(0xff);
    k[12] = byte_of(0xff);
    k[13] = byte_of(0xff);
    k[14] = byte_of(0xff);
    k[15] = byte_of(0xff);
    k[16] = byte_of(0xff);
    k[17] = byte_of(0xff);
    k[18] = byte_of(0xff);
    k[19] = byte_of(0xff);
    k[20] = byte_of(0xff);
    k[21] = byte_of(0xff);
    k[22] = byte_of(0xff);
    k[23] = byte_of(0xff);
    k[24] = byte_of(0xff);
    k[25] = byte_of(0xff);
    k[26] = byte_of(0xff);
    k[27] = byte_of(0xff);
    k[28] = byte_of(0xff);
    k[29] = byte_of(0xff);
    k[30] = byte_of(0xff);
    k[31] = byte_of(0x7f);
    return k;
}

// (p + 3) / 8, the exponent point decompression's square root uses.
static exp_sqrt_const: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0xfe);
    k[1] = byte_of(0xff);
    k[2] = byte_of(0xff);
    k[3] = byte_of(0xff);
    k[4] = byte_of(0xff);
    k[5] = byte_of(0xff);
    k[6] = byte_of(0xff);
    k[7] = byte_of(0xff);
    k[8] = byte_of(0xff);
    k[9] = byte_of(0xff);
    k[10] = byte_of(0xff);
    k[11] = byte_of(0xff);
    k[12] = byte_of(0xff);
    k[13] = byte_of(0xff);
    k[14] = byte_of(0xff);
    k[15] = byte_of(0xff);
    k[16] = byte_of(0xff);
    k[17] = byte_of(0xff);
    k[18] = byte_of(0xff);
    k[19] = byte_of(0xff);
    k[20] = byte_of(0xff);
    k[21] = byte_of(0xff);
    k[22] = byte_of(0xff);
    k[23] = byte_of(0xff);
    k[24] = byte_of(0xff);
    k[25] = byte_of(0xff);
    k[26] = byte_of(0xff);
    k[27] = byte_of(0xff);
    k[28] = byte_of(0xff);
    k[29] = byte_of(0xff);
    k[30] = byte_of(0xff);
    k[31] = byte_of(0x0f);
    return k;
}

// the standard base point B, RFC 8032's own compressed encoding.
static base_point_enc: [byte] {
    let k = alloc_slice[static](32, byte_of(0));
    k[0] = byte_of(0x58);
    k[1] = byte_of(0x66);
    k[2] = byte_of(0x66);
    k[3] = byte_of(0x66);
    k[4] = byte_of(0x66);
    k[5] = byte_of(0x66);
    k[6] = byte_of(0x66);
    k[7] = byte_of(0x66);
    k[8] = byte_of(0x66);
    k[9] = byte_of(0x66);
    k[10] = byte_of(0x66);
    k[11] = byte_of(0x66);
    k[12] = byte_of(0x66);
    k[13] = byte_of(0x66);
    k[14] = byte_of(0x66);
    k[15] = byte_of(0x66);
    k[16] = byte_of(0x66);
    k[17] = byte_of(0x66);
    k[18] = byte_of(0x66);
    k[19] = byte_of(0x66);
    k[20] = byte_of(0x66);
    k[21] = byte_of(0x66);
    k[22] = byte_of(0x66);
    k[23] = byte_of(0x66);
    k[24] = byte_of(0x66);
    k[25] = byte_of(0x66);
    k[26] = byte_of(0x66);
    k[27] = byte_of(0x66);
    k[28] = byte_of(0x66);
    k[29] = byte_of(0x66);
    k[30] = byte_of(0x66);
    k[31] = byte_of(0x66);
    return k;
}


// ---- Field arithmetic over GF(p), p = 2^255 - 19 ----
// Thin wrappers over the bignum toolkit above, fixed to `p_const`.

fn gf_add[&o, &a, &b](o: &!o [byte], a: &a [byte], b: &b [byte]) -> [] int {
    return bn_addmod(o, a, b, p_const);
}

fn gf_sub[&o, &a, &b](o: &!o [byte], a: &a [byte], b: &b [byte]) -> [] int {
    return bn_submod(o, a, b, p_const);
}

fn gf_mul[&o, &a, &b](o: &!o [byte], a: &a [byte], b: &b [byte]) -> [] int {
    return bn_mulmod(o, a, b, p_const);
}

// `a^(p-2) mod p == a^-1 mod p` by Fermat's little theorem, for any
// nonzero `a`. Slower than a dedicated inversion algorithm and far
// simpler to have gotten right, which is this module's own trade
// throughout.
fn gf_invert[&o, &a](o: &!o [byte], a: &a [byte]) -> [] int {
    return bn_modpow(o, a, exp_inv_const, p_const);
}

// `a^((p+3)/8) mod p` -- point decompression's own square-root step
// (§ below); a candidate square root, corrected against `sqrt_m1_const`
// when it is the wrong one of the two square roots `p`'s residues have.
fn gf_pow2523[&o, &a](o: &!o [byte], a: &a [byte]) -> [] int {
    return bn_modpow(o, a, exp_sqrt_const, p_const);
}

// The low bit of a fully-reduced field element's canonical encoding --
// point decompression's own parity check, and the sign bit a compressed
// point's top byte carries.
fn gf_is_odd[&a](a: &a [byte]) -> [] int {
    return int_of(a[0]) & 1;
}

// ---- Points on the twisted Edwards curve, extended coordinates ----
//
// A point is one 128-byte buffer: X = buf[0..32], Y = buf[32..64],
// Z = buf[64..96], T = buf[96..128] (the extended-coordinates identity
// X*Y = T*Z, RFC 8032's own representation). One buffer rather than
// four separate ones so a point is a single region parameter to pass
// around, not four kept in sync by hand.

// The unified addition/doubling formula (works for `p == q` too, which
// is how doubling is done throughout this module -- one formula, no
// separate doubling code path to keep in sync with it). Checked
// independently in Python before any of this was written: repeated
// doubling-and-adding of the base point by the group order returns the
// identity.
fn point_add[&p, &q, &o](p: &p [byte], q: &q [byte], o: &!o [byte]) -> [] int {
    region r {
        let ta = alloc_slice[r](32, byte_of(0));
        let tb = alloc_slice[r](32, byte_of(0));
        let a = alloc_slice[r](32, byte_of(0));
        let b = alloc_slice[r](32, byte_of(0));
        let c = alloc_slice[r](32, byte_of(0));
        let d = alloc_slice[r](32, byte_of(0));
        let e = alloc_slice[r](32, byte_of(0));
        let f = alloc_slice[r](32, byte_of(0));
        let g = alloc_slice[r](32, byte_of(0));
        let h = alloc_slice[r](32, byte_of(0));

        gf_sub(ta, p[32..64], p[0..32]);
        gf_sub(tb, q[32..64], q[0..32]);
        gf_mul(a, ta, tb);

        gf_add(ta, p[32..64], p[0..32]);
        gf_add(tb, q[32..64], q[0..32]);
        gf_mul(b, ta, tb);

        gf_mul(ta, p[96..128], q[96..128]);
        gf_mul(tb, ta, d_const);
        gf_add(c, tb, tb);

        gf_mul(ta, p[64..96], q[64..96]);
        gf_add(d, ta, ta);

        gf_sub(e, b, a);
        gf_sub(f, d, c);
        gf_add(g, d, c);
        gf_add(h, b, a);

        gf_mul(o[0..32], e, f);
        gf_mul(o[32..64], g, h);
        gf_mul(o[96..128], e, h);
        gf_mul(o[64..96], f, g);
    }
    return 0;
}

fn point_copy[&p, &o](p: &p [byte], o: &!o [byte]) -> [] int {
    bn_copy_into(o, p);
    return 0;
}

// The neutral element (0, 1, 1, 0) in extended coordinates.
fn point_identity[&o](o: &!o [byte]) -> [] int {
    var i = 0;
    while i < 128 {
        o[i] = byte_of(0);
        i = i + 1;
    }
    o[32] = byte_of(1);
    o[64] = byte_of(1);
    return 0;
}

// `o = s * p`, double-and-add from the most significant bit of `s`
// down. `s` is a 32-byte little-endian scalar; every bit is walked
// (leading zero bits just double the identity, which is the identity),
// the same "correct rather than fastest" trade `bn_modpow` already
// makes.
fn point_scalarmult[&o, &p, &s](o: &!o [byte], p: &p [byte], s: &s [byte]) -> [] int {
    region r {
        let result = alloc_slice[r](128, byte_of(0));
        point_identity(result);
        let scratch = alloc_slice[r](128, byte_of(0));

        var bit = len(s) * 8 - 1;
        while bit >= 0 {
            point_add(result, result, scratch);
            point_copy(scratch, result);

            let byte_idx = bit / 8;
            let bit_idx = bit % 8;
            let bset = (int_of(s[byte_idx]) >> bit_idx) & 1;
            if bset == 1 {
                point_add(result, p, scratch);
                point_copy(scratch, result);
            }
            bit = bit - 1;
        }
        point_copy(result, o);
    }
    return 0;
}

// `o = s * B`, the fixed base point -- `point_scalarmult` against
// `base_point_enc` decompressed once. Kept as its own entry point
// because every signing and verifying operation needs exactly this,
// never an arbitrary second base.
fn point_scalarmult_base[&o, &s](o: &!o [byte], s: &s [byte]) -> [] int {
    region r {
        let b = alloc_slice[r](128, byte_of(0));
        point_unpack(base_point_enc, b);
        point_scalarmult(o, b, s);
    }
    return 0;
}

// Compress an extended point to its 32-byte encoding: affine `y`, with
// affine `x`'s low bit folded into the top bit of the last byte.
fn point_pack[&p, &o](p: &p [byte], o: &!o [byte]) -> [] int {
    region r {
        let zinv = alloc_slice[r](32, byte_of(0));
        gf_invert(zinv, p[64..96]);
        let x = alloc_slice[r](32, byte_of(0));
        let y = alloc_slice[r](32, byte_of(0));
        gf_mul(x, p[0..32], zinv);
        gf_mul(y, p[32..64], zinv);
        bn_copy_into(o, y);
        if gf_is_odd(x) == 1 {
            o[31] = byte_of(int_of(o[31]) | 0x80);
        }
    }
    return 0;
}

// Decompress a 32-byte encoding into an extended point. Returns `1` on
// success, `0` on a malformed encoding: a non-canonical `y` (`>= p`,
// RFC 8032 §5.1.3's own rejection), or a `y` for which `x^2` has no
// square root at all (the encoding does not name a point on the curve).
fn point_unpack[&enc, &o](enc: &enc [byte], o: &!o [byte]) -> [] int {
    var ok = 0;
    region r {
        let y = alloc_slice[r](32, byte_of(0));
        bn_copy_into(y, enc);
        let sign = (int_of(y[31]) >> 7) & 1;
        y[31] = byte_of(int_of(y[31]) & 0x7f);

        if bn_compare(y, p_const) < 0 {
            let y2 = alloc_slice[r](32, byte_of(0));
            gf_mul(y2, y, y);
            let one = alloc_slice[r](32, byte_of(0));
            one[0] = byte_of(1);
            let num = alloc_slice[r](32, byte_of(0));
            gf_sub(num, y2, one);
            let dy2 = alloc_slice[r](32, byte_of(0));
            gf_mul(dy2, d_const, y2);
            let den = alloc_slice[r](32, byte_of(0));
            gf_add(den, dy2, one);
            let deninv = alloc_slice[r](32, byte_of(0));
            gf_invert(deninv, den);
            let x2 = alloc_slice[r](32, byte_of(0));
            gf_mul(x2, num, deninv);

            let cand = alloc_slice[r](32, byte_of(0));
            gf_pow2523(cand, x2);
            let check = alloc_slice[r](32, byte_of(0));
            gf_mul(check, cand, cand);

            var have_root = 0;
            if bn_compare(check, x2) == 0 {
                have_root = 1;
            } else {
                let alt = alloc_slice[r](32, byte_of(0));
                gf_mul(alt, cand, sqrt_m1_const);
                gf_mul(check, alt, alt);
                if bn_compare(check, x2) == 0 {
                    bn_copy_into(cand, alt);
                    have_root = 1;
                }
            }

            if have_root == 1 {
                if gf_is_odd(cand) != sign {
                    let negated = alloc_slice[r](32, byte_of(0));
                    let zero = alloc_slice[r](32, byte_of(0));
                    gf_sub(negated, zero, cand);
                    bn_copy_into(cand, negated);
                }
                bn_copy_into(o[0..32], cand);
                bn_copy_into(o[32..64], y);
                let one2 = alloc_slice[r](32, byte_of(0));
                one2[0] = byte_of(1);
                bn_copy_into(o[64..96], one2);
                gf_mul(o[96..128], cand, y);
                ok = 1;
            }
        }
    }
    return ok;
}

fn point_equal[&p, &q](p: &p [byte], q: &q [byte]) -> [] int {
    region r {
        let zp = alloc_slice[r](32, byte_of(0));
        let zq = alloc_slice[r](32, byte_of(0));
        gf_invert(zp, p[64..96]);
        gf_invert(zq, q[64..96]);
        let xp = alloc_slice[r](32, byte_of(0));
        let xq = alloc_slice[r](32, byte_of(0));
        let yp = alloc_slice[r](32, byte_of(0));
        let yq = alloc_slice[r](32, byte_of(0));
        gf_mul(xp, p[0..32], zp);
        gf_mul(xq, q[0..32], zq);
        gf_mul(yp, p[32..64], zp);
        gf_mul(yq, q[32..64], zq);
        if bn_compare(xp, xq) == 0 {
            if bn_compare(yp, yq) == 0 {
                return 1;
            }
        }
    }
    return 0;
}

import std.crypto;

// RFC 8032 §5.1.5's clamp: clear the low three bits (a multiple of the
// cofactor 8, keeping the scalar in the prime-order subgroup), clear
// bit 255, set bit 254. Specified by the RFC itself, not derived here.
fn clamp[&a](a: &!a [byte]) -> [] int {
    a[0] = byte_of(int_of(a[0]) & 0xf8);
    a[31] = byte_of((int_of(a[31]) & 0x7f) | 0x40);
    return 0;
}

// The public key for a 32-byte seed: `SHA-512(seed)`'s first half,
// clamped into the secret scalar `a`, times the base point.
pub fn public_key_from_seed[&seed, &o](seed: &seed [byte], o: &!o [byte]) -> [] int {
    region r {
        let h = alloc_slice[r](64, byte_of(0));
        crypto.sha512(seed, h);
        let a = alloc_slice[r](32, byte_of(0));
        bn_copy_into(a, h[0..32]);
        clamp(a);
        let point = alloc_slice[r](128, byte_of(0));
        point_scalarmult_base(point, a);
        point_pack(point, o);
    }
    return 0;
}

// `seed` (32 bytes) and `msg` (any length), producing a 64-byte
// detached signature into `o`: `R_enc || S`, RFC 8032 §5.1.6.
pub fn sign[&seed, &msg, &o](seed: &seed [byte], msg: &msg [byte], o: &!o [byte]) -> [] int {
    region r {
        let h = alloc_slice[r](64, byte_of(0));
        crypto.sha512(seed, h);
        let a = alloc_slice[r](32, byte_of(0));
        bn_copy_into(a, h[0..32]);
        clamp(a);
        let prefix = alloc_slice[r](32, byte_of(0));
        bn_copy_into(prefix, h[32..64]);

        let point_a = alloc_slice[r](128, byte_of(0));
        point_scalarmult_base(point_a, a);
        let pk = alloc_slice[r](32, byte_of(0));
        point_pack(point_a, pk);

        let mlen = len(msg);
        let buf1 = alloc_slice[r](32 + mlen, byte_of(0));
        bn_copy_into(buf1[0..32], prefix);
        bn_copy_into(buf1[32..32 + mlen], msg);
        let rhash = alloc_slice[r](64, byte_of(0));
        crypto.sha512(buf1, rhash);
        let rscalar = alloc_slice[r](32, byte_of(0));
        bn_reduce_wide(rscalar, rhash, l_const);

        let point_r = alloc_slice[r](128, byte_of(0));
        point_scalarmult_base(point_r, rscalar);
        let r_enc = alloc_slice[r](32, byte_of(0));
        point_pack(point_r, r_enc);

        let buf2 = alloc_slice[r](64 + mlen, byte_of(0));
        bn_copy_into(buf2[0..32], r_enc);
        bn_copy_into(buf2[32..64], pk);
        bn_copy_into(buf2[64..64 + mlen], msg);
        let khash = alloc_slice[r](64, byte_of(0));
        crypto.sha512(buf2, khash);
        let kscalar = alloc_slice[r](32, byte_of(0));
        bn_reduce_wide(kscalar, khash, l_const);

        let ka = alloc_slice[r](32, byte_of(0));
        bn_mulmod(ka, kscalar, a, l_const);
        let s = alloc_slice[r](32, byte_of(0));
        bn_addmod(s, rscalar, ka, l_const);

        bn_copy_into(o[0..32], r_enc);
        bn_copy_into(o[32..64], s);
    }
    return 0;
}

// `1` if `sig` (64 bytes) is a valid Ed25519 signature by `pk` (32
// bytes) over `msg`, `0` otherwise -- a malformed `pk`, a non-canonical
// `S` (RFC 8032 §5.1.7's own rejection), a malformed `R`, or the
// signature equation itself not holding.
pub fn verify[&pk, &msg, &sig](pk: &pk [byte], msg: &msg [byte], sig: &sig [byte]) -> [] int {
    var ok = 0;
    region r {
        let s = alloc_slice[r](32, byte_of(0));
        bn_copy_into(s, sig[32..64]);
        if bn_compare(s, l_const) < 0 {
            let point_a = alloc_slice[r](128, byte_of(0));
            let a_ok = point_unpack(pk, point_a);
            if a_ok == 1 {
                let mlen = len(msg);
                let buf2 = alloc_slice[r](64 + mlen, byte_of(0));
                bn_copy_into(buf2[0..32], sig[0..32]);
                bn_copy_into(buf2[32..64], pk);
                bn_copy_into(buf2[64..64 + mlen], msg);
                let khash = alloc_slice[r](64, byte_of(0));
                crypto.sha512(buf2, khash);
                let kscalar = alloc_slice[r](32, byte_of(0));
                bn_reduce_wide(kscalar, khash, l_const);

                let sb = alloc_slice[r](128, byte_of(0));
                point_scalarmult_base(sb, s);

                let ka = alloc_slice[r](128, byte_of(0));
                point_scalarmult(ka, point_a, kscalar);

                let point_r = alloc_slice[r](128, byte_of(0));
                let r_ok = point_unpack(sig[0..32], point_r);
                if r_ok == 1 {
                    let rhs = alloc_slice[r](128, byte_of(0));
                    point_add(point_r, ka, rhs);
                    if point_equal(sb, rhs) == 1 {
                        ok = 1;
                    }
                }
            }
        }
    }
    return ok;
}
