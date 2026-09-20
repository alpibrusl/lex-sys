// Shared borrows nest (§5). Freezing is not exclusive: `frozen` says the
// value may not move or change, and two readers do neither.
//
// Both references are live at once and they have different regions, because
// each `borrow` block introduces its own. That costs nothing here -- each is
// used where its own region is expected.
//~ STDOUT 33
//~ EXIT 0

struct Bytes {
    len: int,
}

fn len_of[&r](b: &r Bytes) -> [] int {
    return b.len;
}

fn main() -> [io] int {
    let buf = Bytes { len: 3 };

    borrow buf as &a in {
        borrow buf as &b in {
            putchar(48 + len_of(a));
            putchar(48 + len_of(b));
        }
    }

    putchar(10);
    return 0;
}
