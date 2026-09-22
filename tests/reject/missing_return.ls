//~ ERROR can finish without returning a value
//~ RULE missing-return

fn main() -> [] int {
    if true {
        return 0;
    }
}
