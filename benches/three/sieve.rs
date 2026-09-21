// The same algorithm as `benches/sieve_checked.ls`, in Rust.
// Built with `-C overflow-checks=on`, to match lex-sys's semantics.

const LIMIT: usize = 60000;
const ROUNDS: i64 = 1000;

#[inline(never)]
fn run() -> i64 {
    let mut mark = vec![0u8; LIMIT];
    let mut found = 0i64;
    for _ in 0..ROUNDS {
        for m in mark.iter_mut() {
            *m = 0;
        }
        let mut p = 2usize;
        while p * p < LIMIT {
            if mark[p] == 0 {
                let mut m = p * p;
                while m < LIMIT {
                    mark[m] = 1;
                    m += p;
                }
            }
            p += 1;
        }
        found = 0;
        for i in 2..LIMIT {
            if mark[i] == 0 {
                found += 1;
            }
        }
    }
    found
}

fn main() {
    println!("{}", run());
}
