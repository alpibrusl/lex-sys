module fmt.text;

// `docs/modules.md` — a module is a **namespace**, and nothing else.
//
// Nothing in this file grants anything. `print_nat` takes an `&!i Io`
// because writing to the console needs the capability that authorises
// it, and its row says `[io_write]` because that is what it did. Being
// `pub` changes neither: §6 is emphatic that a module is *not* a trust
// boundary, and `pub` means reachable, never safe.
//
// The other half of that is what makes this file worth having. The
// functions here are byte-for-byte the ones 25 other files in this
// repository each define for themselves -- and moving them here changed
// **no hash**, not their own and not any caller's, because a call
// encodes the callee's hash rather than its spelling (§2).

pub fn write_all[&r, &i](io: &!i Io, s: &r [byte]) -> [io_write] int {
    var n = 0;
    while n < len(s) {
        putchar(io, int_of(s[n]));
        n = n + 1;
    }
    return len(s);
}

pub fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {
    if n >= 10 {
        print_nat(io, n / 10);
    }
    return putchar(io, 48 + n % 10);
}

pub fn newline[&i](io: &!i Io) -> [io_write] int {
    return putchar(io, 10);
}
