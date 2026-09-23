//! The benchmark programs: every variant of each computes the same answer.

use super::*;

/// `benches/` — both halves of every pair compute the same answer.
///
/// The benchmarks are how `docs/overflow-cost.md` knows what the overflow
/// trap costs, and the measurement is only meaningful while the two halves
/// are the same program. Each one returns `result - expected`, so a
/// non-zero exit is a wrong answer — which is what this checks.
///
/// **Not a timing gate.** Wall-clock in CI is noise, and a benchmark that
/// fails the build when a runner is busy teaches people to ignore it.
/// `scripts/bench.py` is where the numbers come from; this is only here so
/// a refactor cannot quietly make the two halves disagree.
#[test]
fn every_benchmark_pair_agrees() {
    let dir = repo_root().join("benches");
    let scratch = scratch("benches");
    let mut pairs = 0;

    for name in ["sum", "sieve", "scan", "fib", "reduce"] {
        for half in ["checked", "wrapping"] {
            let source = dir.join(format!("{name}_{half}.ls"));
            let exe = scratch.join(format!("{name}_{half}"));
            let build = Command::new(BIN)
                .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
                .output()
                .expect("the compiler runs");
            assert!(
                build.status.success(),
                "`{name}_{half}` should compile, but the compiler said:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );

            let run = Command::new(&exe).output().expect("the benchmark runs");
            assert_eq!(
                run.status.code(),
                Some(0),
                "`{name}_{half}` computed the wrong answer (it exits with its error)"
            );
        }
        pairs += 1;
    }

    // Counted from the directory rather than written down twice: a pair
    // added to `benches/` and forgotten here would otherwise never run.
    let on_disk = std::fs::read_dir(&dir)
        .expect("benches/ is readable")
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.ends_with("_checked.ls"))
        .count();
    assert_eq!(pairs, on_disk, "every pair in `benches/` should be covered here");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `benches/guards.c` — all three modes of every kernel compute the same
/// answer.
///
/// `docs/check-cost.md` and `docs/poison.md` read a cost out of the gaps
/// between unchecked, trapping and poison, and a gap means "what the
/// check costs" only while the three are the same program. If a check
/// ever *fires*, the number measured is the trap rather than the check,
/// and the kernel's data needs fixing rather than its timing believing —
/// which already happened once, to `byte_of`, whose first fill held a
/// `-1`.
///
/// Poison has a second way to be wrong that trapping does not: its
/// operation has to stay **defined** where the check would have fired —
/// a masked shift, a truncated byte, a clamped conversion — so a
/// disagreement here is as likely to be a bad fallback as bad data.
///
/// Built with a small `ROUNDS`, because the shape of the loop is what the
/// measurement is about and that does not depend on how many times the
/// outer one goes round. Not a timing gate, for the reason
/// `every_benchmark_pair_agrees` gives.
#[test]
fn every_guard_kernel_agrees_across_its_three_modes() {
    let source = repo_root().join("benches").join("guards.c");
    let scratch = scratch("guards");
    let mut kernels = 0;

    for kernel in 0..=11 {
        let mut answers = Vec::new();
        for mode in 0..=2 {
            let exe = scratch.join(format!("k{kernel}m{mode}"));
            let cc = Command::new("cc")
                .args([
                    "-O2".as_ref(),
                    "-DROUNDS=2".as_ref(),
                    format!("-DKERNEL={kernel}").as_ref(),
                    format!("-DMODE={mode}").as_ref(),
                    source.as_os_str(),
                    "-o".as_ref(),
                    exe.as_os_str(),
                ])
                .output()
                .expect("a C compiler");
            assert!(
                cc.status.success(),
                "kernel {kernel} mode={mode} should compile:\n{}",
                String::from_utf8_lossy(&cc.stderr)
            );
            let run = Command::new(&exe).output().expect("the kernel runs");
            assert_eq!(
                run.status.code(),
                Some(0),
                "kernel {kernel} mode={mode} should exit 0; a check that fires means \
                 the data trips it, so the gap would measure the trap"
            );
            answers.push(String::from_utf8_lossy(&run.stdout).into_owned());
        }
        assert!(
            answers.windows(2).all(|pair| pair[0] == pair[1]),
            "kernel {kernel}: unchecked, trapping and poison must compute the same thing, \
             and they printed {answers:?}"
        );
        kernels += 1;
    }

    // One `noinline run` per kernel. Counting the `#elif KERNEL ==` lines
    // instead would over-count: `main` switches on the same macro to fill
    // each kernel's data.
    let text = std::fs::read_to_string(&source).expect("guards.c is readable");
    let on_disk = text.matches("__attribute__((noinline))").count();
    assert_eq!(kernels, on_disk, "every kernel in `guards.c` should be covered here");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// `benches/three/` — every build still computes the same answer.
///
/// `docs/against-c-and-rust.md` compares lex-sys to C and Rust on the
/// same algorithm, and the comparison means nothing unless the three
/// sources really are the same algorithm. Each prints a checksum; this
/// checks they agree.
///
/// **Not a timing gate.** Wall-clock in CI is noise, and
/// `scripts/three.py` is where the numbers come from. This runs each
/// build once, so a drifted translation is a red build rather than a
/// quietly wrong table.
///
/// The C and Rust halves are skipped when their compilers are not on the
/// path. The lex-sys halves always run.
#[test]
fn the_three_language_benchmarks_agree() {
    let dir = repo_root().join("benches").join("three");
    let scratch = scratch("three");

    /// Build, run once, and hand back what it printed.
    fn checksum(exe: &Path) -> String {
        let run = Command::new(exe).output().expect("the benchmark runs");
        assert_eq!(run.status.code(), Some(0), "`{}` exited badly", exe.display());
        String::from_utf8_lossy(&run.stdout).trim().to_owned()
    }

    let cc = ["cc", "clang", "gcc"].into_iter().find(|c| which(c));
    let have_rustc = which("rustc");

    // (lex-sys source, C source, Rust source, extra C flags)
    let groups: [(&str, &str, &str, &[&str]); 3] = [
        ("mandelbrot.ls", "mandelbrot.c", "mandelbrot.rs", &["-DCHECKED=1"]),
        ("sieve.ls", "sieve.c", "sieve.rs", &[]),
        // Two files per language, because the boundary is the point
        // (`docs/purity.md` §1). The C half is linked below.
        ("purity.ls", "purity.c", "purity.rs", &["-DPROMISED=0"]),
    ];

    for (index, (ours, in_c, in_rust, flags)) in groups.iter().enumerate() {
        let exe = scratch.join(format!("ls{index}"));
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                dir.join(ours).as_os_str(),
                "--std".as_ref(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{ours}` should compile, but the compiler said:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let expected = checksum(&exe);
        assert!(!expected.is_empty(), "`{ours}` printed no checksum");

        // A benchmark split across a compilation boundary has its other
        // half beside it, named `<stem>_lib.<extension>`.
        let library = |source: &str| -> Option<PathBuf> {
            let (stem, extension) = source.rsplit_once('.').expect("a source file has a suffix");
            let candidate = dir.join(format!("{stem}_lib.{extension}"));
            candidate.exists().then_some(candidate)
        };

        if let Some(cc) = cc {
            let exe = scratch.join(format!("c{index}"));
            let build = Command::new(cc)
                .args(["-O2"])
                .args(*flags)
                .arg(dir.join(in_c))
                .args(library(in_c))
                .arg("-o")
                .arg(&exe)
                .output()
                .expect("the C compiler runs");
            assert!(
                build.status.success(),
                "`{in_c}` should compile:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            assert_eq!(checksum(&exe), expected, "`{in_c}` disagrees with `{ours}`");
        }

        if have_rustc {
            let exe = scratch.join(format!("rs{index}"));
            let mut link: Vec<String> = Vec::new();
            if let Some(half) = library(in_rust) {
                let archive = scratch.join(format!("librs{index}.a"));
                assert!(
                    Command::new("rustc")
                        .args(["-O", "--crate-type=staticlib"])
                        .arg(&half)
                        .arg("-o")
                        .arg(&archive)
                        .status()
                        .expect("rustc runs")
                        .success(),
                    "`{}` should compile",
                    half.display()
                );
                link = vec![
                    "-L".into(),
                    scratch.display().to_string(),
                    "-l".into(),
                    format!("static=rs{index}"),
                ];
            }
            let build = Command::new("rustc")
                .args(["-O", "-Coverflow-checks=on"])
                .arg(dir.join(in_rust))
                .args(&link)
                .arg("-o")
                .arg(&exe)
                .output()
                .expect("rustc runs");
            assert!(
                build.status.success(),
                "`{in_rust}` should compile:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
            assert_eq!(checksum(&exe), expected, "`{in_rust}` disagrees with `{ours}`");
        }
    }

    // The f64 pair has no lex-sys half — that is §4's whole point — so the
    // two of them are checked against each other.
    if let (Some(cc), true) = (cc, have_rustc) {
        let in_c = scratch.join("cf64");
        let in_rust = scratch.join("rsf64");
        assert!(
            Command::new(cc)
                .args(["-O2"])
                .arg(dir.join("mandelbrot_f64.c"))
                .arg("-o")
                .arg(&in_c)
                .status()
                .expect("the C compiler runs")
                .success()
        );
        assert!(
            Command::new("rustc")
                .arg("-O")
                .arg(dir.join("mandelbrot_f64.rs"))
                .arg("-o")
                .arg(&in_rust)
                .status()
                .expect("rustc runs")
                .success()
        );
        assert_eq!(
            checksum(&in_c),
            checksum(&in_rust),
            "the two f64 builds disagree, so §4's precision figure is not what it says"
        );
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Is `name` an executable on `PATH`?
fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// `docs/benchmarks-game.md` §2 — every ported program prints the answer
/// the Benchmarks Game publishes.
///
/// The published value is the point. Two programs I wrote agreeing with
/// each other proves only that I made the same mistake twice; agreeing
/// with a number someone else published is evidence. Each is run at the
/// size the Game states an answer for, which is small enough to be a
/// test rather than a benchmark — `scripts/game.py` runs the same
/// binaries at the sizes that take seconds.
#[test]
fn benchmark_game_programs_print_the_published_answer() {
    let root = repo_root().join("benches").join("game");
    let cases: [(&str, &str, &str); 3] = [
        ("fannkuch", "7", "228\nPfannkuchen(7) = 16\n"),
        ("spectral", "100", "1.274219991\n"),
        (
            "binarytrees",
            "10",
            "stretch tree of depth 11\t check: 4095\n\
             1024\t trees of depth 4\t check: 31744\n\
             256\t trees of depth 6\t check: 32512\n\
             64\t trees of depth 8\t check: 32704\n\
             16\t trees of depth 10\t check: 32752\n\
             long lived tree of depth 10\t check: 2047\n",
        ),
    ];

    let dir = scratch("benchmark-game");
    for (name, size, expected) in cases {
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                "--std".as_ref(),
                root.join(format!("{name}.ls")).as_os_str(),
                "-o".as_ref(),
                exe.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).arg(size).output().expect("the program runs");
        assert_eq!(run.status.code(), Some(0), "`{name}` should exit 0");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "`{name}` at N={size} should print what the Benchmarks Game publishes"
        );

        // And the C counterpart, which is only a fair comparison if it
        // computes the same thing (§2's rule).
        let c_exe = dir.join(format!("{name}_c"));
        let cc = Command::new("cc")
            .args([
                "-O2".as_ref(),
                root.join(format!("{name}.c")).as_os_str(),
                "-o".as_ref(),
                c_exe.as_os_str(),
            ])
            .output()
            .expect("a C compiler");
        assert!(cc.status.success(), "{}", String::from_utf8_lossy(&cc.stderr));
        let c_run = Command::new(&c_exe).arg(size).output().expect("the C program runs");
        assert_eq!(
            String::from_utf8_lossy(&c_run.stdout),
            expected,
            "`{name}.c` must compute the same thing, or the timing means nothing"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
