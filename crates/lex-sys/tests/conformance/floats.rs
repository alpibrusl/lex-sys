//! Floating point: printing, `sqrt`, and NaN.

use super::*;

/// The corpus the printer is checked against, and the driver that prints
/// it (`docs/float-printing.md` §5).
///
/// The expected output *is* the literal that was written. `{:e}` is the
/// shortest decimal that reads back to the same bits, and the lexer's
/// `f64::from_str` is correctly rounded, so a program that prints back
/// what it was given agrees with the oracle by construction -- and one
/// that does not has disagreed about digits, not about notation.
fn float_corpus() -> Vec<f64> {
    let mut values: Vec<f64> = Vec::new();

    // Every normal power of two. These are the values with *uneven*
    // neighbours -- the gap below is half the gap above -- and they are
    // the case a printer gets wrong first, so none of them is sampled.
    for e in 1..=2046u64 {
        values.push(f64::from_bits(e << 52));
    }
    // Every subnormal power of two, down to the smallest float there is,
    // and the all-ones mantissa beside each: the top and the bottom of
    // every subnormal binade.
    for i in 0..52 {
        values.push(f64::from_bits(1u64 << i));
        values.push(f64::from_bits((1u64 << (i + 1)) - 1));
    }
    // Every power of ten in range, where the decimal and the binary
    // grids line up worst.
    for k in -307..=308 {
        values.push(format!("1e{k}").parse().expect("a power of ten in range"));
    }
    // The ones with a reputation.
    for text in [
        "0.1",
        "0.3",
        "0.5",
        "1.0",
        "100.0",
        "1e23",
        "9.999999999999999e22",
        "2.9802322387695312e-8",
        "1.7976931348623157e308",
        "2.2250738585072014e-308",
        "5e-324",
        "3.141592653589793",
        "2.718281828459045",
        "1.1125369292536007e-308",
    ] {
        values.push(text.parse().expect("a float in range"));
    }

    // And a deterministic spread of bit patterns, so the corpus is not
    // only the cases someone thought of. splitmix64 rather than a
    // dependency: the seed is fixed, so a failure here reproduces.
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    while values.len() < 9000 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let candidate = f64::from_bits(z);
        if candidate.is_finite() {
            values.push(candidate);
        }
    }
    values
}

const SHOW_LS: &str = "\
module main;

import std.fmt;
import std.io;

fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    region a {
        let buf = alloc_slice[a](24, byte_of(0));
        let n = fmt.float_into(buf, x);
        io.write_all(i, buf[0..n]);
        io.newline(i);
    }
    return 0;
}
";

/// Build and run a program that prints `values`, one per line.
fn print_floats(tag: &str, values: &[f64]) -> Vec<String> {
    let mut source = String::from(SHOW_LS);
    source.push_str("\nfn main(world: World) -> [] int {\n");
    source.push_str("    let Split { io, ffi, fs, heap, args } = split(world);\n");
    source.push_str("    release(args); release(heap); release(fs); release(ffi);\n");
    source.push_str("    borrow mut io as &!i in {\n");
    for value in values {
        // `{:e}` is the shortest round-tripping form, which is both a
        // literal the lexer reads back exactly and the line the program
        // should print.
        source.push_str(&format!("        show(i, {value:e});\n"));
    }
    source.push_str("    }\n    release(io);\n    return 0;\n}\n");

    let dir = scratch(tag);
    let path = dir.join("corpus.ls");
    std::fs::write(&path, &source).expect("a writable fixture");
    let exe = dir.join("corpus");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "the corpus program should compile:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "the corpus program should exit 0");
    let lines: Vec<String> =
        String::from_utf8_lossy(&run.stdout).lines().map(str::to_owned).collect();
    let _ = std::fs::remove_dir_all(&dir);
    lines
}

#[test]
fn shortest_printing_agrees_with_an_oracle() {
    let values = float_corpus();
    let lines = print_floats("float-corpus", &values);
    assert_eq!(lines.len(), values.len(), "one line per value");

    let mut wrong = Vec::new();
    for (value, line) in values.iter().zip(&lines) {
        let expected = format!("{value:e}");
        if &expected != line {
            wrong.push(format!("{expected} printed as {line}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} values printed differently from the oracle; first few:\n{}",
        wrong.len(),
        values.len(),
        wrong.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The three values that have no literal, and the signed zero. They are
/// spelled rather than refused (`docs/floating-point.md` §2), and the
/// spellings are the oracle's.
#[test]
fn the_values_with_no_literal_are_spelled() {
    let source = format!(
        "{SHOW_LS}\nfn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi);\n\
         \x20   let huge = 1.0e308;\n\
         \x20   let infinite = huge * 10.0;\n\
         \x20   let nothing = 0.0;\n\
         \x20   borrow mut io as &!i in {{\n\
         \x20       show(i, infinite);\n\
         \x20       show(i, -infinite);\n\
         \x20       show(i, infinite - infinite);\n\
         \x20       show(i, nothing / nothing);\n\
         \x20       show(i, -nothing);\n\
         \x20   }}\n\
         \x20   release(io);\n\
         \x20   return 0;\n\
         }}\n"
    );
    let dir = scratch("float-specials");
    let path = dir.join("specials.ls");
    std::fs::write(&path, &source).expect("a writable fixture");
    let exe = dir.join("specials");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    let expected = format!(
        "{:e}\n{:e}\n{:e}\n{:e}\n{:e}\n",
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::NAN,
        -0.0f64
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A buffer too short is answered with -1 rather than a trap or a partial
/// line: the caller chose the buffer, so the caller hears about it.
#[test]
fn a_short_buffer_is_refused_rather_than_overrun() {
    let source = "\
module main;

import std.fmt;
import std.io;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    var code = 0;
    region a {
        let small = alloc_slice[a](4, byte_of(0));
        if fmt.float_into(small, 3.141592653589793) == 0 - 1 {
            code = 7;
        }
        let enough = alloc_slice[a](24, byte_of(0));
        if fmt.float_into(enough, 3.141592653589793) != 19 {
            code = 9;
        }
    }
    release(io);
    return code;
}
";
    let dir = scratch("float-short-buffer");
    let path = dir.join("short.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("short");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(7), "a short buffer answers -1, a long one the length");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/differential.md` §4 — the one value that was not the same on
/// every target.
///
/// IEEE-754 leaves the sign and payload of a *generated* NaN to the
/// hardware, and x86-64 sets the sign where aarch64 does not, so
/// `bits_of(0.0 / 0.0)` printed `-2251799813685248` on one CI runner and
/// `9221120237041090560` on the other. Every way this language can make
/// a NaN is tried here, at run time and folded, and all of them must read
/// back as the one pattern.
#[test]
fn every_nan_has_one_bit_pattern() {
    let source = "\
import std.io;
fn show[&i](i: &!i Io, x: float) -> [io_write] int {
    io.print_int(i, bits_of(x));
    io.newline(i);
    return 0;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    var zero = 0.0;
    var one = 1.0;
    var inf = 1.0 / 0.0;
    borrow mut io as &!i in {
        show(i, 0.0 / 0.0);
        show(i, -(0.0 / 0.0));
        show(i, zero / zero);
        show(i, -(zero / zero));
        show(i, inf - inf);
        show(i, inf * zero);
        show(i, sqrt(-one));
        show(i, (zero / zero) + one);
        show(i, -((zero / zero) * one));
    }
    release(io);
    return 0;
}
";
    let dir = scratch("one-nan");
    let path = dir.join("nan.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("nan");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 9);
    for line in lines {
        assert_eq!(line, "9221120237041090560", "every NaN reads as 0x7ff8000000000000");
    }

    // And the reason the canonicalisation exists, measured on the machine
    // running this test rather than asserted: the hardware's own NaN.
    // Where it already is the canonical pattern, `bits_of` changes
    // nothing; where it is not, the program above would have printed the
    // other one.
    let hardware = (std::hint::black_box(0.0_f64) / std::hint::black_box(0.0_f64)).to_bits();
    if cfg!(target_arch = "x86_64") {
        assert_eq!(hardware, 0xfff8_0000_0000_0000, "x86-64's indefinite NaN has its sign set");
    }
    if cfg!(target_arch = "aarch64") {
        assert_eq!(hardware, 0x7ff8_0000_0000_0000, "aarch64's default NaN is positive");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/float-math.md` §2: `sqrt` is correctly rounded, and the two
/// hand-rolled roots it replaced were not.
///
/// Checked against Rust's own `f64::sqrt` — the same instruction, so
/// this is a test that the builtin reaches it rather than a test of the
/// hardware. The values include the four exponents where the twenty-step
/// Newton loop in `benches/game/spectral.ls` was wrong by 10^43 and
/// more, which is the failure this replaced.
#[test]
fn sqrt_agrees_with_the_hardware() {
    let dir = scratch("float-sqrt");

    // A deterministic spread: the specials, a decade sweep, and values
    // across the exponent range where the old loop fell short.
    let mut values: Vec<f64> = vec![0.0, 1.0, 2.0, 0.25, 1e-300, 1e-8, 1e8, 1e100, 1e200, 1e300];
    let mut seed = 0x5eed_u64;
    for _ in 0..2000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mantissa = f64::from(((seed >> 11) & 0xff_ffff) as u32) / 16_777_216.0;
        let exponent = ((seed >> 40) % 600) as i32 - 300;
        values.push(libm_ldexp(mantissa + 0.5, exponent));
    }

    /// `mantissa * 2^exponent`, without pulling in a dependency.
    fn libm_ldexp(mantissa: f64, exponent: i32) -> f64 {
        let mut out = mantissa;
        let mut n = exponent;
        while n > 0 {
            out *= 2.0;
            n -= 1;
        }
        while n < 0 {
            out /= 2.0;
            n += 1;
        }
        out
    }

    // The program prints `sqrt` of each value, shortest-round-trip, one
    // per line — so a disagreement in the last bit is visible.
    let mut program = String::from(
        "import std.fmt;\nimport std.io;\n\n\
         fn show[&i](i: &!i Io, x: float) -> [io_write] int {\n\
         \x20   region a {\n\
         \x20       let out = alloc_slice[a](32, byte_of(0));\n\
         \x20       let n = fmt.float_into(out, x);\n\
         \x20       io.write_all(i, out[0..n]);\n\
         \x20   }\n\
         \x20   return io.newline(i);\n\
         }\n\n\
         fn main(world: World) -> [] int {\n\
         \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
         \x20   release(ffi); release(fs); release(heap); release(args);\n\
         \x20   borrow mut io as &!i in {\n",
    );
    for v in &values {
        program.push_str(&format!("        show(i, sqrt({v:e}));\n"));
    }
    program.push_str("    }\n    release(io);\n    return 0;\n}\n");

    let source = dir.join("sqrt.ls");
    std::fs::write(&source, &program).expect("the program is written");
    let exe = dir.join("sqrt");
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

    let out = Command::new(&exe).output().expect("it runs");
    let printed = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), values.len(), "one line per value");

    for (line, value) in lines.iter().zip(&values) {
        let got: f64 = line.parse().unwrap_or_else(|_| panic!("`{line}` is not a float"));
        let want = value.sqrt();
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "sqrt({value:e}) came back {got:e}, and the hardware says {want:e}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
