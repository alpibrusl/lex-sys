// `slow_pure` in its own crate. Rust has no purity attribute at all, so
// there is no `PROMISED` half of this file to write.
#[no_mangle]
pub extern "C" fn slow_pure(x: i64) -> i64 {
    let mut acc: i64 = 0;
    for i in 0..64i64 {
        acc = acc.wrapping_mul(31).wrapping_add(x ^ i);
    }
    acc
}
