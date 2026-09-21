// The caller. See `purity_lib.rs`: there is nothing to declare.
unsafe extern "C" {
    fn slow_pure(x: i64) -> i64;
}

#[inline(never)]
fn run(rounds: i64) -> i64 {
    let mut total: i64 = 0;
    for _ in 0..rounds {
        unsafe {
            total = total.wrapping_add(slow_pure(7)).wrapping_add(slow_pure(7));
        }
    }
    total
}

fn main() {
    println!("{}", run(2000000));
}
