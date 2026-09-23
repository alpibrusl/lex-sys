//! Standard output, standard error and standard input.

use super::*;

/// `docs/bulk-io.md` §3.2 — a faster program is not a more powerful one.
///
/// This is the whole argument of the slice, so it is pinned byte for
/// byte rather than field by field: the same program written with
/// `putchar` and with `write_bytes` must produce an *identical*
/// authority report. If a future change gave the bulk primitive its own
/// effect label, its own capability, or a `foreign_symbols` entry for
/// the `fwrite` it lowers to, this fails — and it should, because §2's
/// complaint was precisely that the fast path used to cost more
/// authority than the slow one.
///
/// `foreign_symbols` staying empty is the subtle half. `write_bytes`
/// does call libc, but the program neither declared that call nor can
/// choose it; it is the builtin's implementation, the same way `putchar`
/// has always been libc's. A report that named `fwrite` here would be
/// telling a reader to audit something they cannot influence.
#[test]
fn bulk_output_costs_exactly_what_one_byte_costs() {
    const PROLOGUE: &str = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
";
    let per_byte = format!(
        "{PROLOGUE}    borrow mut io as &!i in {{ putchar(i, 104); putchar(i, 105); }}\n    release(io);\n    return 0;\n}}\n"
    );
    let bulk = format!(
        "{PROLOGUE}    borrow mut io as &!i in {{ write_bytes(i, \"hi\"); }}\n    release(io);\n    return 0;\n}}\n"
    );

    let slow = authority_json(&per_byte, "bulk-authority-putchar");
    let fast = authority_json(&bulk, "bulk-authority-write");
    assert_eq!(slow, fast, "`write_bytes` must report exactly what `putchar` reports (§3.2)");
    assert!(slow.contains("\"effects\": [\"io_write\"]"), "and that is `io_write`:\n{slow}");
    assert!(
        slow.contains("\"foreign_symbols\": []"),
        "a builtin's own libc call is not the program reaching foreign code:\n{slow}"
    );
}

/// §3 — `write_bytes` goes through the same stream `putchar` does.
///
/// The reason the primitive lowers to `fwrite` on `stdout` rather than
/// POSIX `write` on descriptor 1: `putchar` is buffered by stdio, so a
/// raw descriptor write would have jumped the queue and the two kinds of
/// output would interleave in the wrong order. Nothing about the types
/// catches that — only running it does. The fixture writes a strictly
/// increasing sequence through alternating primitives, so any reordering
/// shows up as an out-of-order digit rather than as a subtle diff.
#[test]
fn bulk_and_per_byte_output_share_one_stream() {
    let dir = scratch("bulk-ordering");
    let path = dir.join("ordering.ls");
    let source = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        var round = 0;
        while round < 2000 {
            putchar(i, 48);
            write_bytes(i, \"12\");
            putchar(i, 51);
            write_bytes(i, \"456\");
            putchar(i, 55);
            write_bytes(i, \"89\");
            round = round + 1;
        }
    }
    release(io);
    return 0;
}
";
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("ordering");
    let build = Command::new(BIN)
        .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "it exits cleanly");
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "0123456789".repeat(2000),
        "the two primitives must interleave in program order, not in stream order"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/utf8.md` §3 — the decoder agrees with an independent oracle.
///
/// The fixture next door pins ten named cases; this pins the *rule* over
/// inputs nobody chose, the way `shortest_printing_agrees_with_an_oracle`
/// does for floats. Rust's own `String::from_utf8_lossy` implements the
/// same maximal-subpart rule §3.2 adopts, and `str::from_utf8` the same
/// strict validity §3.1 adopts, so both columns have a reference that is
/// not this repository.
///
/// §2 is why the oracle is not GNU `wc -m`: it counts `f5 80 80 80` as
/// one character, and that sequence encodes a value above U+10FFFF.
#[test]
fn utf8_decoding_agrees_with_an_oracle() {
    let dir = scratch("utf8-oracle");
    let program = dir.join("oracle.ls");
    std::fs::write(
        &program,
        "\
import std.io;
import std.utf8;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi);
    borrow mut io as &!i in {
        region a {
            var buf = alloc_slice[a](4096, byte_of(0));
            var more = true;
            while more {
                var n = 0;
                var k = 0;
                var eof = false;
                while k < 4 {
                    let c = getchar(i);
                    if c < 0 { eof = true; k = 4; }
                    else { n = n | (c << (8 * k)); k = k + 1; }
                }
                if eof { more = false; }
                else {
                    var j = 0;
                    while j < n { buf[j] = byte_of(getchar(i)); j = j + 1; }
                    let text = buf[0..n];
                    io.print_int(i, utf8.count(text));
                    io.write_all(i, \" \");
                    if utf8.is_valid(text) { io.write_all(i, \"1\"); }
                    else { io.write_all(i, \"0\"); }
                    io.newline(i);
                }
            }
        }
    }
    release(io);
    return 0;
}
",
    )
    .expect("a writable fixture");

    let exe = dir.join("oracle");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            program.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // Deterministic inputs across four shapes: well-formed text at every
    // width, raw noise, valid text with bytes corrupted, and valid text
    // cut short. The last two are where a decoder's skip rule shows.
    let mut seed: u64 = 0x5eed_1234_9abc_def0;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let sample = "héllo wörld 日本語 😀🎉 αβγ";
    let mut cases: Vec<Vec<u8>> = Vec::new();
    for i in 0..2000u32 {
        let mut c: Vec<u8> = Vec::new();
        match i % 4 {
            0 => {
                for _ in 0..(next() % 12 + 1) {
                    let p = match next() % 4 {
                        0 => next() % 0x80,
                        1 => 0x80 + next() % (0x800 - 0x80),
                        2 => 0x800 + next() % (0x1_0000 - 0x800),
                        _ => 0x1_0000 + next() % (0x11_0000 - 0x1_0000),
                    } as u32;
                    if let Some(ch) = char::from_u32(p) {
                        let mut b = [0u8; 4];
                        c.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                    }
                }
            }
            1 => {
                for _ in 0..(next() % 16 + 1) {
                    c.push((next() % 256) as u8);
                }
            }
            2 => {
                c.extend_from_slice(sample.as_bytes());
                for _ in 0..(next() % 3 + 1) {
                    let at = (next() as usize) % c.len();
                    c[at] = (next() % 256) as u8;
                }
            }
            _ => {
                let b = sample.as_bytes();
                let take = (next() as usize) % b.len() + 1;
                c.extend_from_slice(&b[..take]);
            }
        }
        if !c.is_empty() {
            cases.push(c);
        }
    }

    let mut input: Vec<u8> = Vec::new();
    let mut expected = String::new();
    for c in &cases {
        input.extend_from_slice(&(c.len() as u32).to_le_bytes());
        input.extend_from_slice(c);
        // The oracle: Rust's own decoder, for both columns.
        let count = String::from_utf8_lossy(c).chars().count();
        let valid = u8::from(std::str::from_utf8(c).is_ok());
        expected.push_str(&format!("{count} {valid}\n"));
    }

    let mut child = Command::new(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the program runs");
    {
        use std::io::Write;
        child.stdin.take().expect("stdin").write_all(&input).expect("the write lands");
    }
    let out = child.wait_with_output().expect("it finishes");
    let got = String::from_utf8_lossy(&out.stdout);

    let mismatch = got
        .lines()
        .zip(expected.lines())
        .enumerate()
        .find(|(_, (g, e))| g != e)
        .map(|(i, (g, e))| (i, g.to_owned(), e.to_owned()));
    assert!(
        mismatch.is_none(),
        "case {:?} disagrees with Rust: got {:?}, expected {:?} (bytes {:02x?})",
        mismatch.as_ref().map(|m| m.0),
        mismatch.as_ref().map(|m| m.1.clone()),
        mismatch.as_ref().map(|m| m.2.clone()),
        mismatch.as_ref().map(|m| cases[m.0].clone()),
    );
    assert_eq!(got.lines().count(), cases.len(), "every case should have answered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §1.2, which is the measurement the whole
/// design turns on.
///
/// Standard output is fully buffered when it is not a terminal, and a
/// trap does not flush it — so a program that says what is wrong and
/// then dies says nothing at all. Measured before this slice at **zero
/// bytes**, into a file and through a pipe both.
///
/// C guarantees `stderr` is not fully buffered, so the same message on
/// the other stream has already left. That is a property of the stream
/// this compiler picked rather than of anything lex-sys does, which is
/// exactly why it is worth a test: the day the backend reaches for
/// POSIX `write`, or a different stream, this is what notices.
#[test]
fn a_diagnostic_survives_a_trap() {
    let dir = scratch("stderr-trap");
    let source = dir.join("trap.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
        \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
        \x20   release(ffi); release(fs); release(heap); release(args);\n\
        \x20   var bad = 0;\n\
        \x20   borrow mut io as &!i in {\n\
        \x20       write_bytes(i, \"on stdout\\n\");\n\
        \x20       write_err(i, \"on stderr\\n\");\n\
        \x20       // `byte_of` traps outside 0..255 (`strings.md` §2).\n\
        \x20       bad = int_of(byte_of(300));\n\
        \x20   }\n\
        \x20   release(io);\n\
        \x20   return bad;\n\
        }\n",
    )
    .expect("the fixture is written");

    let exe = dir.join("trap");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("it runs");
    assert!(!run.status.success(), "the fixture is supposed to trap");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "on stderr\n",
        "the diagnostic did not survive the trap"
    );
    // The other half, and the reason the first half matters: the same
    // message on the output stream is gone.
    assert!(
        run.stdout.is_empty(),
        "standard output flushed on a trap, which would make §1.2's argument moot: {:?}",
        String::from_utf8_lossy(&run.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §4: the negative half of the authority report
/// must not lie about a program whose entire output is a diagnostic.
///
/// A new label with no entry in the report's table produces exactly one
/// wrong sentence — "never touches the console" — about a program that
/// touches nothing else. `authority.md` §2.2 is why that is the half
/// worth guarding: an absent label is a proof, and a proof of the wrong
/// thing is worse than no report.
#[test]
fn a_diagnostic_only_program_touches_the_console() {
    let dir = scratch("stderr-authority");
    let source = dir.join("complain.ls");
    std::fs::write(
        &source,
        "fn shout[&i](io: &!i Io) -> [err_write] int {\n\
        \x20   return write_err(io, \"only a diagnostic\\n\");\n\
        }\n\
        \n\
        fn main(world: World) -> [] int {\n\
        \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
        \x20   release(ffi); release(fs); release(heap); release(args);\n\
        \x20   borrow mut io as &!i in { shout(i); }\n\
        \x20   release(io);\n\
        \x20   return 1;\n\
        }\n",
    )
    .expect("the fixture is written");

    let report =
        Command::new(BIN).arg("authority").arg(&source).output().expect("the compiler runs");
    assert!(report.status.success(), "{}", String::from_utf8_lossy(&report.stderr));
    let text = String::from_utf8_lossy(&report.stdout);

    assert!(text.contains("err_write"), "the report should name the label:\n{text}");
    assert!(
        !text.contains("the console"),
        "a program whose whole output is a diagnostic touches the console:\n{text}"
    );
    // The rest of the negative half still holds, so the entry narrowed
    // the claim rather than removing it.
    for absent in ["the filesystem", "the heap", "the command line", "foreign code"] {
        assert!(text.contains(absent), "the report should still say `{absent}`:\n{text}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
