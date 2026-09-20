// Functions see each other regardless of definition order.
//~ STDOUT z
//~ EXIT 0

fn main() -> [io] int {
    putchar(last_letter());
    putchar(10);
    return 0;
}

fn last_letter() -> [] int {
    return 122;
}
