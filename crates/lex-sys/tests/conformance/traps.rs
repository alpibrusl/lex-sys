//! Defined behaviour: every run-time check traps where C would be undefined, and nowhere else.

use super::*;

#[test]
fn byte_of_traps_outside_a_byte_rather_than_truncating() {
    // `docs/strings.md` §2: truncation is the silently wrong answer
    // `defined-behaviour.md` §2.1 already refused for `+`. One unsigned
    // comparison covers both ends, so `byte_of(-1)` dies with `byte_of(256)`.
    for value in ["256", "0 - 1"] {
        let dir = scratch(&format!("byte-range-{}", value.replace([' ', '-'], "")));
        let source = dir.join("byte.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     return int_of(byte_of({value}));\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("byte");

        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert!(!run.status.success(), "`byte_of({value})` should not succeed");
        assert_eq!(
            run.status.code(),
            None,
            "`byte_of({value})` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn indexing_past_a_slice_traps_rather_than_reading_on() {
    // `docs/defined-behaviour.md` §1: the alternative to a bounds check is
    // reading past the end of an allocation, and this language has no
    // undefined behaviour to do that in. One unsigned comparison covers
    // both ends -- a negative index read as unsigned is enormous -- so the
    // check below catches `xs[5]` and `xs[-1]` with the same instruction.
    for index in ["5", "0 - 1"] {
        let dir = scratch(&format!("slice-bounds-{}", index.replace([' ', '-'], "")));
        let source = dir.join("bounds.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     var n = 0;\n\
                     region a {{ let xs = alloc_slice[a](3, 7); n = xs[{index}]; }}\n\
                     return n;\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("bounds");

        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert!(!run.status.success(), "`xs[{index}]` should not succeed");
        assert_eq!(run.status.code(), None, "`xs[{index}]` should be killed by a signal, not exit");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `docs/slicing.md` §2: a range past the end traps.
///
/// The same rule indexing has, applied to the operation that produces a
/// range rather than an element -- and it has to be, because the
/// alternative is a slice claiming a length its allocation does not
/// have, which is a buffer overrun with a type on it.
#[test]
fn slicing_past_the_end_traps() {
    // One unsigned comparison covers both ends, as it does for an index:
    // a negative bound read as unsigned is enormous.
    for range in ["0..13", "0 - 1..3"] {
        let tag = format!("slice-bounds-{}", range.replace([' ', '-', '.'], ""));
        let dir = scratch(&tag);
        let source = dir.join("bounds.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     let text = \"hello, world\";\n\
                     return len(text[{range}]);\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join("bounds");
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            None,
            "`text[{range}]` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// §2 again: `a > b` stops rather than yielding empty.
///
/// An inverted range is a bug in the program that wrote it, and quietly
/// returning nothing is the defined-but-wrong answer
/// `defined-behaviour.md` §2.1 refuses.
#[test]
fn an_inverted_range_traps() {
    let dir = scratch("slice-inverted");
    let source = dir.join("inverted.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let text = \"hello, world\";\n\
             return len(text[5..2]);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("inverted");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), None, "`text[5..2]` should be killed by a signal");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn integer_overflow_traps_rather_than_wrapping() {
    // `docs/defined-behaviour.md` §2.1. Wrapping would be *defined* -- C has
    // it for unsigned, Rust has it in release -- so it is not undefined
    // behaviour that is being refused here, it is a silently wrong answer.
    // The wrong answer propagates; the stopped process does not.
    let dir = scratch("integer-overflow");
    let source = dir.join("overflow.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             var n = 9223372036854775807;\n\
             return n + 1;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("overflow");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "overflow should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exhausting_an_arena_traps_rather_than_running_past_the_chunk() {
    // §6: an arena is one chunk, obtained once and released once, which is
    // what makes release O(1). Asking it for more than it has is therefore
    // possible -- and it *traps*, because the alternative to a trap is
    // writing past the end of an allocation, and this language does not have
    // undefined behaviour to do that in (#1).
    let dir = scratch("arena-exhaustion");
    let source = dir.join("exhaust.ls");
    std::fs::write(
        &source,
        "struct Node { value: int }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); release(io);\n\
             region a {\n\
                 var i = 0;\n\
                 while i < 20000 {\n\
                     let node = alloc[a](Node { value: i });\n\
                     i = i + 1;\n\
                 }\n\
             }\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("exhaust");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "an exhausted arena should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn division_by_zero_traps_rather_than_being_undefined() {
    let dir = scratch("divide-by-zero");
    let source = dir.join("divzero.ls");
    std::fs::write(
        &source,
        "fn divide(a: int, b: int) -> [] int { return a / b; }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi);\n\
             release(io);\n\
             return divide(1, 0);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("divzero");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    // A trap, not a silently wrong answer and not undefined behaviour: the
    // process dies rather than continuing with nonsense (#1).
    assert!(!run.status.success(), "division by zero should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/emitted-checks.md` §4 — the three division traps nothing tested.
///
/// `division_by_zero_traps_rather_than_being_undefined` above covers
/// `1 / 0` and predates this. The other three were rules in
/// `defined-behaviour.md` §2.3 that no test had ever run, and reading the
/// emitted code found one of them false — so these exist to make the
/// section falsifiable on **both** targets rather than on the one a
/// disassembly happened to be taken on.
///
/// The operands come out of variables, because literals are folded and a
/// certain trap written down is a compile error rather than a run
/// (`compile-time.md` §4). §4.2 is what happens when those two paths
/// disagree.
#[test]
fn the_other_division_traps() {
    let dir = scratch("division-traps");
    // `(source, should it die)`.
    let cases = [("1", "0", "%", true), ("-9223372036854775808", "-1", "/", true)];

    for (a, b, op, dies) in cases {
        let name = if op == "%" { "rem" } else { "div" };
        let source = dir.join(format!("{name}.ls"));
        std::fs::write(
            &source,
            format!(
                "fn op(a: int, b: int) -> [] int {{ return a {op} b; }}\n\
                 fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     var x = {a}; var y = {b};\n\
                     return op(x, y);\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            if dies { None } else { Some(0) },
            "`{a} {op} {b}` should {}",
            if dies { "be killed by a signal" } else { "exit 0" }
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/emitted-checks.md` §4.1 and §4.2 — `a % -1` is 0, both ways.
///
/// `defined-behaviour.md` §2.3 said `int::MIN % -1` traps, borrowing the
/// quotient's reason for an operator that produces no quotient. It does
/// not: the emitted code is `cmp $-1` and a `mov $0`, and 0 is the right
/// answer.
///
/// Both spellings in one program on purpose. The constant folder had
/// Rust's `checked_rem` rule and the backend had the hardware's, so the
/// same expression was a compile error written down and a 0 computed.
/// This fails if either half moves.
#[test]
fn a_remainder_by_minus_one_is_zero() {
    let dir = scratch("rem-minus-one");
    let source = dir.join("rem.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             let folded = -9223372036854775808 % -1;\n\
             var a = -9223372036854775808; var b = -1;\n\
             let computed = a % b;\n\
             return folded + computed;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("rem");

    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "folding `int::MIN % -1` should agree with running it:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0), "both spellings should answer 0");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `std.math` §3.3: `abs` traps on the most negative integer.
///
/// Every other language's `abs` returns the negative number here, which
/// is the silently-wrong answer this one exists to refuse. It does not
/// return at all -- and the trap is `0 - n` doing what `-` already does
/// rather than a check bolted on, so it costs nothing on every other
/// input.
#[test]
fn abs_of_the_most_negative_integer_traps() {
    let dir = scratch("std-abs-traps");
    let source = dir.join("abs.ls");
    std::fs::write(
        &source,
        "import std.math;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return math.abs(-9223372036854775808);\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("abs");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            source.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "`abs(int::MIN)` must not succeed");
    assert_eq!(run.status.code(), None, "it is killed by a signal, not an exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_boxed_slice_checks_its_own_size() {
    // `docs/boxed-slices.md` §3. Two runtime rules, both of which would
    // otherwise reserve less memory than is about to be written.
    //
    // A negative count is not a small allocation, it is a mistake -- and
    // `s[0]` of one would read memory nobody reserved. A `count * stride`
    // that overflows is the same mistake arrived at by arithmetic, which
    // is why it is checked for the reason every other multiplication is.
    for count in ["0 - 3", "4611686018427387904"] {
        let dir = scratch("boxed-slice-size");
        let source = dir.join("size.ls");
        std::fs::write(
            &source,
            format!(
                "fn main(world: World) -> [] int {{\n\
                     let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
                     release(args); release(fs); release(ffi); release(io);\n\
                     var n = 0;\n\
                     borrow mut heap as &!h in {{\n\
                         let b = box_slice(h, {count}, 0);\n\
                         n = unbox_slice(h, b);\n\
                     }}\n\
                     release(heap);\n\
                     return n;\n\
                 }}\n"
            ),
        )
        .expect("a writable fixture");

        let exe = dir.join("size");
        let build = Command::new(BIN)
            .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

        let run = Command::new(&exe).output().expect("the compiled program runs");
        assert_eq!(
            run.status.code(),
            None,
            "`box_slice(h, {count}, 0)` should be killed by a signal, not exit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `docs/bitwise.md` §3 and §4 — what a shift does at the edges.
///
/// Not reject fixtures: a trapping program is one that compiled, and the
/// reject harness runs `check` (`slicing.md` §8 made the same correction).
///
/// The fourth case is the one that is easy to get wrong in the other
/// direction. `1 << 63` sets the sign bit, which read as arithmetic is an
/// overflow — and §4 says a shift is bits, so it is the answer.
#[test]
fn a_shift_past_the_width_traps() {
    let scratch = scratch("bitwise-shift");
    // (expression, does it trap). A trap is `ud2`, so the process is killed
    // by a signal and has no exit code — which is how every other trapping
    // test here states it.
    //
    // The amount comes through a `var` rather than as a literal, and that
    // is load-bearing now: `docs/compile-time.md` §4 refuses a shift
    // whose operands are both literals *at compile time*, so writing
    // `1 << 64` here would test the diagnostic instead of the trap.
    // `a_certain_trap_is_refused_at_compile_time` is the other half.
    let cases = [
        ("64", true),
        ("0 - 1", true),
        // Cranelift's `ishl` masks the amount, so an unguarded lowering
        // would make `1 << 65` into `1 << 1` and hand back 2. It traps
        // instead.
        ("65", true),
        ("63", false),
        ("0", false),
    ];

    for (index, (expression, traps)) in cases.iter().enumerate() {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var seen = 0;\n\
             \x20   var amount = {expression};\n\
             \x20   if (1 << amount) != 12345 {{ seen = 0; }}\n\
             \x20   if (1 >> amount) != 12345 {{ seen = 0; }}\n\
             \x20   return seen;\n\
             }}\n"
        );
        let path = scratch.join(format!("shift{index}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = scratch.join(format!("shift{index}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "a shift by `{expression}` should compile — the amount is a runtime value:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if *traps {
            assert_eq!(
                run.status.code(),
                None,
                "`{expression}` should be killed by a signal, not exit"
            );
        } else {
            assert_eq!(run.status.code(), Some(0), "`{expression}` should not trap");
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `docs/floating-point.md` §4 — `truncate` traps exactly where C is
/// undefined.
///
/// NaN, both infinities, and any magnitude at or past `2^63`. C says the
/// behaviour is undefined; this says the process stops, which is §2.1's
/// rule applying where it belongs — the result would be a number, and
/// there is no number it could honestly be.
///
/// Not reject fixtures, for `slicing.md` §8's reason: a trapping program
/// is one that compiled.
#[test]
fn truncate_traps_where_c_is_undefined() {
    let scratch = scratch("float-truncate");
    // (expression, does it trap)
    let cases = [
        ("0.0 / 0.0", true),
        ("1.0 / 0.0", true),
        ("0.0 - 1.0 / 0.0", true),
        ("1.0e30", true),
        ("-1.0e30", true),
        // `2^63` is about 9.223e18, so this is inside and that is not.
        ("9.0e18", false),
        ("1.0e19", true),
        ("0.0", false),
        ("-2.7", false),
    ];

    for (index, (expression, traps)) in cases.iter().enumerate() {
        let source = format!(
            "fn main(world: World) -> [] int {{\n\
             \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var seen = truncate({expression});\n\
             \x20   if seen == 12345 {{ seen = 0; }}\n\
             \x20   return 0;\n\
             }}\n"
        );
        let path = scratch.join(format!("t{index}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = scratch.join(format!("t{index}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`truncate({expression})` should compile — the value is a runtime one:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if *traps {
            assert_eq!(
                run.status.code(),
                None,
                "`truncate({expression})` should be killed by a signal, not exit"
            );
        } else {
            assert_eq!(run.status.code(), Some(0), "`truncate({expression})` should not trap");
        }
    }

    let _ = std::fs::remove_dir_all(&scratch);
}
