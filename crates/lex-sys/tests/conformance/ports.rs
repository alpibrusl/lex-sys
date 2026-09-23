//! The GNU ports -- `base64`, `sort`, `cut` -- held to what the originals print.

use super::*;

/// `examples/base64/` — the port, checked against the program it ports.
///
/// `docs/porting.md`'s claim is that this is GNU coreutils' `base64`, and
/// a claim of that shape is worth what it is tested with. So the test
/// pipes the same bytes through both and compares: twelve input sizes
/// including the ones around the 76-column wrap, both directions, plus the
/// three malformed inputs where the exit status is the whole behaviour.
///
/// The comparison runs against **GNU** coreutils specifically, because
/// that is what `docs/porting.md` claims this is a port of, and the other
/// implementations disagree: macOS ships BSD `base64`, which prints a
/// newline for empty input where GNU prints nothing. Neither is wrong —
/// the port targets one of them, so the test compares against that one.
///
/// Where GNU `base64` is not on the path the comparison is skipped rather
/// than failed. Everything that needs no reference — the round trip, the
/// exit statuses, the megabyte — still runs on every platform, which is
/// why the darwin job is not simply doing less work.
#[test]
fn base64_agrees_with_coreutils() {
    let scratch = scratch("example-base64");
    let exe = scratch.join("base64");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/base64/base64.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`base64` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    /// Feed `input` to `command` on stdin and hand back (stdout, exit code).
    ///
    /// The write goes on its own thread. Writing the whole input first and
    /// only then reading deadlocks as soon as the output exceeds a pipe
    /// buffer: the child blocks writing, so it stops reading, so the
    /// parent blocks writing. Base64 output is larger than its input, so
    /// this is reached by any case past about 48 KiB — which is exactly
    /// the megabyte case at the end, the one that is here to prove the
    /// program streams.
    fn pipe(command: &Path, args: &[&str], input: &[u8]) -> (Vec<u8>, Option<i32>) {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let owned = input.to_vec();
        let writer = std::thread::spawn(move || {
            // A broken pipe is not a failure here: a program that refuses
            // its input exits before reading all of it, which is what the
            // malformed cases below are about.
            let _ = stdin.write_all(&owned);
            drop(stdin);
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (out.stdout, out.status.code())
    }

    // Deterministic rather than random: a failing test should fail the same
    // way twice. The sizes are the boundaries — empty, each remainder mod
    // 3, and either side of the 57 input bytes that fill a 76-column line.
    let sizes = [0usize, 1, 2, 3, 4, 5, 17, 56, 57, 58, 100, 1000];
    let corpus: Vec<Vec<u8>> =
        sizes.iter().map(|n| (0..*n).map(|i| (i * 37 + i / 5) as u8).collect()).collect();

    // `base64 --version` prints "base64 (GNU coreutils) 9.4" on GNU. BSD's
    // has no `--version` at all and exits non-zero, which is the same
    // answer for this purpose: not the program that was ported.
    let reference = Path::new("/usr/bin/base64");
    let have_reference = reference.exists()
        && Command::new(reference)
            .arg("--version")
            // Null stdin, so a version probe can never end up waiting on
            // input it would otherwise inherit from the test runner.
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|v| {
                v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils")
            });

    for (size, input) in sizes.iter().zip(&corpus) {
        let (ours, status) = pipe(&exe, &[], input);
        assert_eq!(status, Some(0), "encoding {size} bytes should succeed");

        if have_reference {
            let (theirs, _) = pipe(reference, &[], input);
            assert_eq!(
                String::from_utf8_lossy(&ours),
                String::from_utf8_lossy(&theirs),
                "encoding {size} bytes differs from GNU coreutils"
            );
        }

        // The round trip holds with or without a reference to compare to.
        let (back, status) = pipe(&exe, &["-d"], &ours);
        assert_eq!(status, Some(0), "decoding {size} bytes should succeed");
        assert_eq!(&back, input, "the round trip lost {size} bytes");
    }

    // Malformed input, where the exit status *is* the behaviour.
    for bad in [&b"abc$def"[..], &b"QQ=A"[..], &b"Q"[..]] {
        let (_, status) = pipe(&exe, &["-d"], bad);
        assert_eq!(status, Some(1), "`{}` should be refused", String::from_utf8_lossy(bad));
        if have_reference {
            let (_, theirs) = pipe(reference, &["-d"], bad);
            assert_eq!(
                status,
                theirs,
                "GNU coreutils disagrees about `{}`",
                String::from_utf8_lossy(bad)
            );
        }
    }

    // An arena is one 64 KiB chunk, so a megabyte through it is the proof
    // that nothing buffers the input (`docs/porting.md` §4).
    let large: Vec<u8> = (0..1_048_576).map(|i| (i * 31 + i / 7) as u8).collect();
    let (encoded, status) = pipe(&exe, &[], &large);
    assert_eq!(status, Some(0), "a megabyte should encode");
    let (decoded, status) = pipe(&exe, &["-d"], &encoded);
    assert_eq!(status, Some(0), "a megabyte should decode");
    assert_eq!(decoded.len(), large.len(), "the round trip changed a megabyte's length");
    assert!(decoded == large, "the round trip changed a megabyte's contents");

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `examples/sort/` — the second port, checked against GNU `sort`.
///
/// The comparison is `LC_ALL=C`, because that is the ordering the program
/// implements: byte order, with no locale anywhere in this language to
/// implement anything else. Same GNU-detection rule as the `base64` test,
/// for the same reason — BSD's `sort` is a different program.
///
/// Where a reference is absent the shape checks still run: the output is
/// still required to be a sorted permutation of the input's lines, which
/// is most of what "sorted" means and needs nobody else's binary.
#[test]
fn sort_agrees_with_gnu_sort() {
    use std::collections::BTreeMap;

    let scratch = scratch("example-sort");
    let exe = scratch.join("sort");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/sort/sort.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`sort` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    /// Run with `input` on stdin and `args` on the command line.
    fn run(command: &Path, args: &[&Path], input: &str) -> (String, Option<i32>) {
        let mut child = Command::new(command)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let owned = input.to_owned();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(owned.as_bytes());
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code())
    }

    /// How many times each line appears, so the output can be checked to
    /// be a rearrangement of the input rather than merely sorted.
    fn multiset(lines: &str) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        // A trailing newline *ends* the last line rather than starting an
        // empty one, which is the one thing a plain `split` gets wrong.
        let body = lines.strip_suffix('\n').unwrap_or(lines);
        if !body.is_empty() {
            for line in body.split('\n') {
                *counts.entry(line).or_default() += 1;
            }
        }
        counts
    }

    let reference = Path::new("/usr/bin/sort");
    let have_reference = reference.exists()
        && Command::new(reference).arg("--version").stdin(Stdio::null()).output().is_ok_and(|v| {
            v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils")
        });

    // The shapes that decide a line sort: nothing, no trailing newline,
    // blank lines, one line a prefix of another, bytes outside the
    // letters, and enough lines to be a real merge.
    let bulk: String = (0..5000)
        .map(|i| format!("{}{}\n", "xyzab".as_bytes()[i % 5] as char, (i * 7919) % 10007))
        .collect();
    let corpus = [
        "",
        "zebra\nant",
        "\n\nb\n\na\n",
        "ab\na\nabc\nb\n",
        "~\n!\nA\na\n0\n",
        "same\nsame\nsame\n",
        &bulk,
    ];

    for input in corpus {
        let (ours, status) = run(&exe, &[], input);
        assert_eq!(status, Some(0), "sorting should succeed");

        // A permutation of the input's lines, in non-descending order.
        // Both halves hold with or without a reference to compare
        // against, which is what the darwin job checks when the `sort`
        // on that machine is not GNU's.
        assert_eq!(
            multiset(&ours),
            multiset(input),
            "the output is not a permutation of the input:\n{ours}"
        );
        let got: Vec<&str> = ours.lines().collect();
        assert!(got.windows(2).all(|w| w[0] <= w[1]), "the output is not in byte order:\n{ours}");

        if have_reference {
            let (theirs, _) = run(reference, &[], input);
            assert_eq!(ours, theirs, "differs from GNU sort on input {input:?}");
        }
    }

    // Named files, several at once, and one that is not there.
    let one = scratch.join("one.txt");
    let two = scratch.join("two.txt");
    std::fs::write(&one, "pear\napple\n").expect("a writable fixture");
    std::fs::write(&two, "fig\nbanana\n").expect("a writable fixture");

    let (ours, status) = run(&exe, &[&one, &two], "");
    assert_eq!(status, Some(0), "sorting two files should succeed");
    assert_eq!(ours, "apple\nbanana\nfig\npear\n", "two files should sort together");
    if have_reference {
        let (theirs, _) = run(reference, &[&one, &two], "");
        assert_eq!(ours, theirs, "differs from GNU sort across two files");
    }

    let missing = scratch.join("not-here.txt");
    let (_, status) = run(&exe, &[&missing], "");
    assert_eq!(status, Some(2), "a missing file should exit 2, as GNU does");

    // Past the 64 KiB first read, so the doubling in `read_file` runs.
    let large: String = (0..40_000).map(|i| format!("line {:06}\n", (i * 31) % 40_000)).collect();
    let big = scratch.join("big.txt");
    std::fs::write(&big, &large).expect("a writable fixture");
    let (ours, status) = run(&exe, &[&big], "");
    assert_eq!(status, Some(0), "a file past the first read should succeed");
    assert_eq!(ours.lines().count(), 40_000, "it lost lines while growing");
    assert!(
        ours.lines().collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]),
        "a file past the first read came out unsorted"
    );
    if have_reference {
        let (theirs, _) = run(reference, &[&big], "");
        assert_eq!(ours, theirs, "differs from GNU sort on a file past the first read");
    }

    // Past the ceiling the growth loop used to stop at.
    //
    // `read_file` *doubled* from 64 KiB until a read came back strictly
    // shorter than the buffer, because `fs_read` cannot report
    // truncation (`docs/file-handles.md` §1). It stopped after eight
    // attempts, so the largest capacity was 8 MiB and *any* file of
    // 8,388,608 bytes or more was refused — under a comment claiming the
    // limit was 16 MiB, which is why nothing caught it. Raising it to
    // fifteen attempts moved the bound to 1 GiB; reading through a
    // **handle** removed it, because `file_read` says when the file is
    // over and nothing has to guess (`porting.md` §10).
    //
    // 9 MB rather than something just over the line, so this keeps
    // testing a file read in many chunks rather than an off-by-one: it
    // is two doublings past where the old bound was, and 130 reads
    // through the handle.
    //
    // `u64` and not the inferred `i32`: 300_000 * 7919 is 2.4 billion,
    // which overflows an `i32`. The first version of this line did not
    // say so, passed under `cargo test --release` where an overflow
    // wraps, and failed on CI, which runs `cargo test` with the checks
    // on. A test for a language whose whole position is that overflow
    // must not wrap silently is a poor place to let one.
    let past_ceiling: String =
        (0..300_000u64).map(|i| format!("{:029}\n", (i * 7919) % 300_000)).collect();
    assert!(past_ceiling.len() > 8 * 1024 * 1024, "the fixture has to clear the old 8 MiB bound");
    let huge = scratch.join("past-ceiling.txt");
    std::fs::write(&huge, &past_ceiling).expect("a writable fixture");
    let (ours, status) = run(&exe, &[&huge], "");
    assert_eq!(status, Some(0), "a file past the old 8 MiB ceiling should now sort");
    assert_eq!(ours.lines().count(), 300_000, "it lost lines past the old ceiling");
    assert!(
        ours.lines().collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]),
        "a file past the old ceiling came out unsorted"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `examples/cut/` — the third port, checked against GNU `cut`.
///
/// It exists as a probe rather than a demonstration: `utf8.md` §1 said
/// the rest of a string library is "code, not design", and the way to
/// find out *which* code is to write a program that needs it. This one
/// asked for `bytes.count_byte` and `bytes.field`, which is how they
/// arrived — the same route `vec.set` and `vec.swap` took.
///
/// The field specs below are the ones where a hand-rolled splitter goes
/// wrong: empty leading and trailing fields, a line with no delimiter at
/// all (which GNU passes through whole without `-s`), and a field past
/// the end.
#[test]
fn cut_agrees_with_gnu_cut() {
    let dir = scratch("example-cut");
    let exe = dir.join("cut");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/cut/cut.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`cut` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let data = "alpha,beta,gamma,delta,epsilon\n\
                one,two,three\n\
                a,b,c,d,e,f,g\n\
                ,leading,empty\n\
                trailing,empty,\n\
                nodelimiterhere\n\
                x,y\n";

    /// Feed `input` on stdin and hand back stdout.
    fn run(cmd: &std::path::Path, args: &[&str], input: &str) -> String {
        use std::io::Write;
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("the program runs");
        let text = input.to_owned();
        let mut stdin = child.stdin.take().expect("stdin");
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
        let out = child.wait_with_output().expect("it finishes");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    // Same rule as the `base64` and `sort` tests: BSD's `cut` is a
    // different program, so the reference only counts when it is GNU's.
    let reference = std::path::Path::new("/usr/bin/cut");
    let have_reference = Command::new(reference)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false);

    for spec in ["-f1", "-f2", "-f2,4", "-f1-3", "-f3-", "-f2-4", "-f1,3-5", "-f9"] {
        let ours = run(&exe, &["-d,", spec], data);
        // Shape checks, which run with or without a reference: every
        // input line produces exactly one output line.
        assert_eq!(
            ours.lines().count(),
            data.lines().count(),
            "`{spec}` should answer one line per input line"
        );
        if have_reference {
            let theirs = run(reference, &["-d,", spec], data);
            assert_eq!(ours, theirs, "`cut -d, {spec}` differs from GNU cut");
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// The failing half of `cut`, against the same reference as the passing
/// half (`docs/standard-error.md` §1.3).
///
/// This replaces an assertion that said `Some(2)` — the status this
/// program happened to have, taken from a source comment claiming it was
/// "the exit status GNU uses", which it was not. The reference was five
/// lines away the whole time and was asked about eight valid specs and
/// never about the invalid one, because a failing run produced nothing
/// to compare. It does now.
#[test]
fn cut_reports_a_bad_field_list_like_gnu_cut() {
    let dir = scratch("example-cut-bad");
    let exe = dir.join("cut");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/cut/cut.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let ours = run_without_locale(&exe, &["-d,", "-fzzz"], "");

    // Unconditional: whatever the reference says, a diagnostic belongs on
    // standard error and nothing belongs on standard output.
    assert!(ours.stdout.is_empty(), "a refused `-f` list should print no data");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        "cut: invalid field value 'zzz'\n",
        "a refused `-f` list should say so on standard error"
    );

    let reference = std::path::Path::new("/usr/bin/cut");
    let have_reference = Command::new(reference)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false);
    if have_reference {
        let theirs = run_without_locale(reference, &["-d,", "-fzzz"], "");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU cut exits differently");
        // Two differences that are not wording, so the comparison is of
        // what follows the program's own name on the first line.
        //
        // GNU adds a second line pointing at `--help`, which this
        // program does not have. And GNU takes its prefix from `argv[0]`,
        // so invoking it by absolute path makes it call itself
        // `/usr/bin/cut` — where this program has a literal.
        // `docs/standard-error.md` §8 is the open question that is, and
        // this is the measurement under it.
        assert_eq!(
            complaint(&ours.stderr),
            complaint(&theirs.stderr),
            "GNU cut words it differently"
        );
    } else {
        assert_eq!(ours.status.code(), Some(1), "a malformed `-f` list should exit 1");
    }

    // The other refusal: an argument this program does not understand.
    // GNU names the option and this does not, so only the status and the
    // stream are comparable — which is still two things that were
    // neither compared nor comparable before (§1.3).
    let unknown = run_without_locale(&exe, &["--nope"], "");
    assert!(unknown.stdout.is_empty(), "an unrecognised argument should print no data");
    assert_eq!(
        String::from_utf8_lossy(&unknown.stderr),
        "cut: usage: cut -d<c> -f<list>\n",
        "an unrecognised argument should say so on standard error"
    );
    if have_reference {
        let theirs = run_without_locale(reference, &["--nope"], "");
        assert_eq!(unknown.status.code(), theirs.status.code(), "GNU cut exits differently");
    } else {
        assert_eq!(unknown.status.code(), Some(1));
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/flags.md` §1 — every spelling GNU accepts, on both ports.
///
/// The gap this closes is not a missing feature. Before `std.flags`, six
/// of these rows disagreed with GNU: `cut` refused four of them loudly,
/// and `base64` answered two of them by **encoding its input and exiting
/// 0**, because `--decode` is not two bytes so the test for `-d` was
/// false and the flag was dropped.
///
/// And every base64 test in this file passed exactly `-d`. Twelve input
/// sizes, both directions and three malformed inputs, all through one
/// spelling of one flag — `line-reading.md` §1's shape again, where the
/// dimension the suite never varied was the length of a line.
///
/// # The expectation is written down, and the reference is a second
/// opinion
///
/// The table carries GNU's answer rather than reading it off whatever
/// `/usr/bin/cut` happens to be, because **macOS ships BSD**: BSD `cut`
/// has no `--delimiter` at all, so comparing against it on darwin tests
/// this program against the wrong specification. The base64 test below
/// already says this about empty input and BSD's extra newline; this is
/// the same fact costing a red build before it was applied here.
///
/// So: the written answer is the assertion, on every target. Where the
/// reference *is* GNU — detected by `--version`, which BSD's does not
/// have — it is checked against the same table, so a wrong expectation
/// fails on linux rather than being believed everywhere.
#[test]
fn both_ports_match_gnu_on_every_spelling() {
    let data = b"a,b,c\n";
    let encoded = b"aGk=\n";

    // `(program, args, stdout, exit, whether GNU agrees)`.
    //
    // The `false` rows are the divergences `flags.md` §4 keeps on
    // purpose. They are asserted to still *disagree*, so one that
    // quietly starts agreeing gets moved rather than forgotten.
    let cases: &[(&str, &[&str], &str, i32, bool)] = &[
        ("cut", &["-d,", "-f2"], "b\n", 0, true),
        ("cut", &["-d", ",", "-f2"], "b\n", 0, true),
        ("cut", &["--delimiter=,", "--fields=2"], "b\n", 0, true),
        ("cut", &["--delimiter", ",", "--fields", "2"], "b\n", 0, true),
        ("cut", &["-d,", "-f", "2"], "b\n", 0, true),
        ("cut", &["-f2", "-d,"], "b\n", 0, true),
        ("cut", &["-d,", "-f1,3"], "a,c\n", 0, true),
        ("cut", &["-d,", "-f2", "--"], "b\n", 0, true),
        ("cut", &["-x"], "", 1, true),
        ("cut", &["-d,,", "-f2"], "", 1, true),
        // §4: no abbreviation, because resolving one needs the option
        // table this design does not have.
        ("cut", &["--delim=,", "-f2"], "", 1, false),
        ("base64", &["-d"], "hi", 0, true),
        ("base64", &["--decode"], "hi", 0, true),
        ("base64", &[], "YUdrPQo=\n", 0, true),
        ("base64", &["--"], "YUdrPQo=\n", 0, true),
        ("base64", &["-q"], "", 1, true),
        // §4: `-i` and `-w` are refused rather than accepted and
        // ignored. Neither is implementable here, and accepting one
        // would be §1's silent lie in a second place.
        ("base64", &["-di"], "", 1, false),
        ("base64", &["-w0"], "", 1, false),
    ];

    let (cut_dir, cut_exe) = build_example("flags-cut", "examples/cut/cut.ls", "cut");
    let (b64_dir, b64_exe) = build_example("flags-base64", "examples/base64/base64.ls", "base64");

    fn feed(command: &Path, args: &[&str], input: &[u8]) -> (String, Option<i32>) {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        let owned = input.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&owned);
            drop(stdin);
        });
        let out = child.wait_with_output().expect("it exits");
        writer.join().expect("the writer thread finishes");
        (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.code())
    }

    /// Whether this reference is GNU coreutils rather than BSD.
    ///
    /// `--version` is the probe because BSD's `cut` does not have it and
    /// GNU's does — the same distinction the table exists for.
    fn is_gnu(path: &Path) -> bool {
        path.exists()
            && Command::new(path)
                .arg("--version")
                .output()
                .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU coreutils"))
    }

    for (program, args, stdout, exit, agrees) in cases {
        let (exe, input): (&Path, &[u8]) = match *program {
            "cut" => (&cut_exe, data),
            _ => (&b64_exe, encoded),
        };
        let written = ((*stdout).to_owned(), Some(*exit));

        assert_eq!(
            feed(exe, args, input),
            written,
            "`{program} {}` should answer what `flags.md` §1 says it does",
            args.join(" ")
        );

        let reference = PathBuf::from(format!("/usr/bin/{program}"));
        if !is_gnu(&reference) {
            continue;
        }
        let theirs = feed(&reference, args, input);
        if *agrees {
            assert_eq!(
                theirs,
                written,
                "GNU `{program} {}` should answer this too — if it does not, \
                 the expectation in this table is wrong rather than the port",
                args.join(" ")
            );
        } else {
            assert_ne!(
                theirs,
                written,
                "`{program} {}` is a documented divergence (`flags.md` §4); \
                 if it now agrees, move the row rather than deleting it",
                args.join(" ")
            );
        }
    }

    let _ = std::fs::remove_dir_all(&cut_dir);
    let _ = std::fs::remove_dir_all(&b64_dir);
}

/// Run a command with no locale, and hand back the whole result.
///
/// `LC_ALL=C` for the reason `sort_agrees_with_gnu_sort` gives — there is
/// no locale anywhere in this language — and here it decides the
/// *wording*, not only the ordering: GNU quotes a bad argument `'zzz'`
/// under POSIX and `‘zzz’` under a UTF-8 locale, so an unpinned
/// comparison passes on one machine and fails on the next. It did:
/// this box defaults to POSIX and the CI runner does not
/// (`docs/standard-error.md` §7.1).
///
/// Ours is run the same way and ignores it, which is the point.
fn run_without_locale(command: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(command)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program runs");
    let text = input.to_owned();
    let mut stdin = child.stdin.take().expect("a piped stdin");
    std::thread::spawn(move || {
        let _ = stdin.write_all(text.as_bytes());
    });
    child.wait_with_output().expect("the program finishes")
}

/// A reference tool's diagnostic, with its own name taken off the front.
///
/// GNU takes that name from `argv[0]`, so a tool invoked by absolute path
/// calls itself `/usr/bin/cut` where this repository's programs carry a
/// literal. That difference is not wording, it is an open question
/// (`docs/standard-error.md` §8), so the comparison is of what follows.
///
/// Only the first line: GNU often adds a second pointing at `--help`.
fn complaint(stream: &[u8]) -> String {
    String::from_utf8_lossy(stream)
        .lines()
        .next()
        .unwrap_or_default()
        .split_once(": ")
        .map_or_else(String::new, |(_, rest)| rest.to_owned())
}

/// Whether the system's tool at `path` is GNU coreutils' rather than
/// BSD's, which is a different program with different wording.
fn is_gnu(path: &Path) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .map(|v| v.status.success() && String::from_utf8_lossy(&v.stdout).contains("GNU coreutils"))
        .unwrap_or(false)
}

/// `docs/standard-error.md` §7, and `file-handles.md` §1.2's other half.
///
/// That section measured a `sort` which answered a file it could not read
/// and a file too large to hold with the same silence, distinguishable
/// only by an exit status: *"still not distinguishable to a person,
/// because neither prints anything"*. One of the two is now, and this is
/// it — the other needs a file past 1 GiB and 1,025 MB of resident
/// memory to reach, so it stays a hand check (§7).
#[test]
fn sort_names_the_file_it_cannot_read() {
    let (dir, exe) = build_example("example-sort-missing", "examples/sort/sort.ls", "sort");
    let missing = dir.join("no-such-file.txt");
    let path = missing.to_string_lossy().into_owned();

    let ours = run_without_locale(&exe, &[&path], "");
    assert_eq!(ours.status.code(), Some(2), "a file that is not there should exit 2");
    assert!(ours.stdout.is_empty(), "a failed read should print no data");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        format!("sort: cannot read: {path}\n"),
        "a failed read should name the file on standard error"
    );

    let reference = Path::new("/usr/bin/sort");
    if is_gnu(reference) {
        let theirs = run_without_locale(reference, &[&path], "");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU sort exits differently");
        // GNU appends the errno string, which `fs_read` does not carry
        // (§6, and `file-handles.md` §3's `Failed(int)` is the shape
        // that would). Everything before it is the same claim.
        assert!(
            complaint(&theirs.stderr).starts_with(&complaint(&ours.stderr)),
            "GNU sort words it differently: {}",
            complaint(&theirs.stderr)
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-error.md` §7: the third program, and the one where the
/// diagnostic is a single line with nothing in it that varies.
///
/// `base64 -d` exits 1 on a byte outside the alphabet — it always did —
/// and said nothing while doing it, which is the row §1 measured at 22
/// bytes for GNU and 0 for this.
#[test]
fn base64_reports_invalid_input_like_gnu_base64() {
    let (dir, exe) = build_example("example-base64-bad", "examples/base64/base64.ls", "base64");

    let ours = run_without_locale(&exe, &["-d"], "a@b\n");
    assert_eq!(ours.status.code(), Some(1), "a byte outside the alphabet should exit 1");
    assert_eq!(
        String::from_utf8_lossy(&ours.stderr),
        "base64: invalid input\n",
        "a refused decode should say so on standard error"
    );

    // And a valid decode stays silent, which is the half a new stream
    // makes it possible to get wrong.
    let good = run_without_locale(&exe, &["-d"], "aGVsbG8K\n");
    assert_eq!(good.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&good.stdout), "hello\n");
    assert!(good.stderr.is_empty(), "a successful decode should say nothing");

    let reference = Path::new("/usr/bin/base64");
    if is_gnu(reference) {
        let theirs = run_without_locale(reference, &["-d"], "a@b\n");
        assert_eq!(ours.status.code(), theirs.status.code(), "GNU base64 exits differently");
        assert_eq!(
            complaint(&ours.stderr),
            complaint(&theirs.stderr),
            "GNU base64 words it differently"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/line-reading.md` §2: a line longer than the buffer.
///
/// The dimension the eight field specs never varied. Before the fix,
/// a 60,000-byte line lost its delimiters, so `cut` switched to the
/// no-delimiter rule and printed 60 KB of the wrong field with exit 0 —
/// a silently wrong answer in a program checked against GNU.
#[test]
fn cut_reports_a_long_line_like_gnu_cut() {
    let (dir, exe) = build_example("example-cut-long", "examples/cut/cut.ls", "cut");

    let reference = Path::new("/usr/bin/cut");
    let have_reference = is_gnu(reference);

    // Around the old 60,000-byte arena slice, and well past it.
    for first in [59_998usize, 59_999, 60_000, 70_000, 200_000] {
        let input = format!("{},second\n", "a".repeat(first));
        let ours = run_without_locale(&exe, &["-d,", "-f2"], &input);
        assert!(ours.status.success(), "a long line should not be an error");
        assert_eq!(
            String::from_utf8_lossy(&ours.stdout),
            "second\n",
            "a {first}-byte first field should not change which field comes back"
        );
        if have_reference {
            let theirs = run_without_locale(reference, &["-d,", "-f2"], &input);
            assert_eq!(
                String::from_utf8_lossy(&ours.stdout),
                String::from_utf8_lossy(&theirs.stdout),
                "GNU cut answers differently on a {first}-byte field"
            );
        }
    }

    // And the field itself, when it is the long one: the whole of it.
    let input = format!("first,{}\n", "b".repeat(100_000));
    let ours = run_without_locale(&exe, &["-d,", "-f2"], &input);
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout).trim_end().len(),
        100_000,
        "a long field should come back whole"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
