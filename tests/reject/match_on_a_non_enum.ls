//~ ERROR `int` cannot be matched
//~ RULE match-on-a-non-enum

fn main() -> [] int {
    match 1 {
        _ => { return 0; }
    }
}
