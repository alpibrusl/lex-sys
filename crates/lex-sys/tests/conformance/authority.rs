//! The authority report, and what it can and cannot prove about a program.

use super::*;

/// `docs/authority.md` §2: the report names what a program performs.
///
/// Three examples whose surfaces differ, so this fails if the union is
/// taken over the wrong set rather than merely if it is empty.
#[test]
fn the_authority_report_names_what_a_program_performs() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        // The console, both directions, and nothing else.
        (
            "tally.ls",
            &["io_read", "io_write"],
            &["the filesystem", "the heap", "the command line", "foreign code"],
        ),
        // Writes only.
        (
            "hello.ls",
            &["io_write"],
            &["the filesystem", "the heap", "the command line", "foreign code"],
        ),
        // Calls into C, and the symbol is named.
        ("pipeline.ls", &["ffi(\"libc\")", "io_write", "labs"], &["the filesystem"]),
    ];

    for (name, performs, never) in cases {
        let path = repo_root().join("examples").join(name);
        let out = Command::new(BIN)
            .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
            .output()
            .expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout);
        for label in *performs {
            assert!(text.contains(label), "`{name}` should perform `{label}`:\n{text}");
        }
        for what in *never {
            assert!(text.contains(what), "`{name}` should never touch {what}:\n{text}");
        }
    }
}

/// §2.1, and the bug that section records.
///
/// `examples/lines.ls` reads `argv` in `main`'s **own body**, and `main`
/// declares `[]` because it owns its capabilities rather than borrowing
/// them. A report built from declared rows said *never touches the
/// command line* about the repository's command-line tool.
#[test]
fn the_authority_report_sees_what_main_does_itself() {
    let path = repo_root().join("examples").join("lines.ls");
    let out = Command::new(BIN)
        .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("args"), "`lines.ls` reads argv in `main`:\n{text}");
    assert!(!text.contains("the command line"), "`lines.ls` does touch the command line:\n{text}");
}

/// §2.2: an absent label is a proof rather than an absence of evidence —
/// the capability was released, and nothing creates another.
#[test]
fn an_unused_capability_never_appears() {
    let dir = scratch("authority-negative");
    let source = dir.join("quiet.ls");
    // Releases everything and performs nothing at all.
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");

    let out = Command::new(BIN)
        .args(["authority".as_ref(), source.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("performs nothing"), "{text}");
    for what in ["the console", "the filesystem", "the heap", "the command line", "foreign code"] {
        assert!(text.contains(what), "`{what}` should be listed as untouched:\n{text}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/budget.md` §5: the report as data, in the shape a supervisor
/// checks against a grant.
///
/// `effects` is the distinct kinds — the coarse question — and `labels`
/// keeps the narrowing for the precise one, which is the line
/// `lex-os-check`'s `CheckReport` already draws for Lex programs.
#[test]
fn the_authority_report_has_a_machine_readable_form() {
    let path = repo_root().join("examples").join("tour.ls");
    let out = Command::new(BIN)
        .args([
            "authority".as_ref(),
            path.as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);

    // Parsed rather than pattern-matched, so a malformed document fails
    // here rather than in whatever reads it next. No JSON dependency in
    // this crate, so the check is structural: balanced braces and
    // brackets, the three keys, and the narrowing carried through.
    assert_eq!(text.matches('{').count(), text.matches('}').count(), "unbalanced braces:\n{text}");
    assert_eq!(
        text.matches('[').count(),
        text.matches(']').count(),
        "unbalanced brackets:\n{text}"
    );
    for key in ["\"effects\"", "\"labels\"", "\"foreign_symbols\""] {
        assert!(text.contains(key), "missing {key}:\n{text}");
    }
    // The coarse kind and the precise argument, both present and distinct.
    assert!(text.contains("\"fs_read\""), "{text}");
    assert!(
        text.contains("{ \"name\": \"fs_read\", \"argument\": \"/tmp\", \"bounded\": true }"),
        "the narrowing should survive:\n{text}"
    );
    // A label that was never narrowed carries an explicit null rather
    // than being absent, so a consumer never has to tell the two apart.
    assert!(
        text.contains("{ \"name\": \"heap\", \"argument\": null, \"bounded\": true }"),
        "an unnarrowed label needs an explicit null:\n{text}"
    );
    assert!(text.contains("\"labs\""), "the foreign symbol should be named:\n{text}");

    // And the human form is unchanged by the flag's existence.
    let plain = Command::new(BIN)
        .args(["authority".as_ref(), path.as_os_str(), "--std".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(plain.status.success());
    let plain = String::from_utf8_lossy(&plain.stdout);
    assert!(plain.contains("fs_read(\"/tmp\")"), "{plain}");
}

/// `docs/purity.md` §2 — the predicate, checked against cases chosen to
/// break it.
///
/// The interesting rows are the ones that are *not* pure despite a `[]`
/// row: `poke` writes through a unique reference, which is `std.vec`'s
/// `set` exactly, and is the whole reason the predicate is two conditions
/// rather than one.
///
/// Each case is a whole program, because pass 2 emits what `main`
/// reaches and nothing else (`standard-library.md` §5.2) — a function
/// nobody calls is not in `Program::funcs` to ask about.
#[test]
fn purity_is_the_row_plus_what_a_reference_may_do() {
    // (program, function name, is it pure)
    let cases: [(&str, &str, bool); 6] = [
        (
            "fn double(x: int) -> [] int { return x + x; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return double(0);\n\
             }\n",
            "double",
            true,
        ),
        // A shared reference is read-only, so reading through one is pure.
        (
            "fn first[&r](s: &r [byte]) -> [] int { return int_of(s[0]); }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return first(\"a\") - 97;\n\
             }\n",
            "first",
            true,
        ),
        // A `[]` row and a unique reference: `std.vec`'s `set` exactly.
        (
            "fn poke[&r](s: &!r [byte]) -> [] int { s[0] = byte_of(1); return 0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var out = 0;\n\
             \x20   region a { let s = alloc_slice[a](2, byte_of(0)); out = poke(s); }\n\
             \x20   return out;\n\
             }\n",
            "poke",
            false,
        ),
        // A unique reference inside a tuple still writes.
        (
            "fn pair[&r](t: (int, &!r [byte])) -> [] int { return t.0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   var out = 0;\n\
             \x20   region a { let s = alloc_slice[a](2, byte_of(0)); out = pair((0, s)); }\n\
             \x20   return out;\n\
             }\n",
            "pair",
            false,
        ),
        // Effects are not pure, however local they look.
        (
            "fn shout[&i](io: &!i Io) -> [io_write] int { putchar(io, 10); return 0; }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi);\n\
             \x20   var out = 0;\n\
             \x20   borrow mut io as &!i in { out = shout(i); }\n\
             \x20   release(io);\n\
             \x20   return out;\n\
             }\n",
            "shout",
            false,
        ),
        // A local arena is invisible from outside, so it does not count.
        (
            "fn scratch(n: int) -> [] int {\n\
             \x20   var total = 0;\n\
             \x20   region a { let s = alloc_slice[a](n, byte_of(1)); total = len(s); }\n\
             \x20   return total;\n\
             }\n\
             fn main(world: World) -> [] int {\n\
             \x20   let Split { io, ffi, fs, heap, args } = split(world);\n\
             \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
             \x20   return scratch(4) - 4;\n\
             }\n",
            "scratch",
            true,
        ),
    ];

    for (source, name, expected) in cases {
        let ast = lex_sys_syntax::parse(source)
            .unwrap_or_else(|d| panic!("`{name}` should parse: {}", d.message));
        let program = lex_sys_ir::lower(&ast)
            .unwrap_or_else(|d| panic!("`{name}` should check: {}", d.message));
        let id = program.find(name).unwrap_or_else(|| panic!("`{name}` should be emitted"));
        assert_eq!(
            program.func(id).is_pure(),
            expected,
            "`{name}` should {}be pure",
            if expected { "" } else { "not " }
        );
    }
}

/// `docs/under-a-grant.md` §2 — a network program reports no network.
///
/// `examples/serve/` binds a TCP port, listens, accepts a connection and
/// answers HTTP. Its effect row is `args` and `ffi`, because sockets are
/// libc and libc is not an authority domain (`reach.md` §5).
///
/// This is pinned rather than merely written down because it is a **gap**,
/// and a gap that nothing observes is one that closes silently. When
/// `reach.md` §6's `Net(host)` row lands, this test fails, and the person
/// who lands it writes the new row here — which is the moment the
/// `lex-os` join in `ROADMAP.md` becomes possible.
#[test]
fn a_network_program_reports_no_network() {
    let (effects, symbols, _) = authority_of("examples/serve/serve.ls");
    assert_eq!(
        effects,
        vec!["args", "ffi"],
        "a program that runs a network server should still report only these \
         two — if it now reports a network label, `under-a-grant.md` §2 and §5 \
         are out of date and so is the roadmap's lex-os row"
    );
    // The symbol list is what covers the difference today, and §3 is why
    // that is a heuristic rather than a wall.
    for expected in ["socket", "bind", "listen", "accept"] {
        assert!(
            symbols.contains(&expected.to_owned()),
            "`{expected}` should be reachable and listed"
        );
    }
}

/// `docs/under-a-grant.md` §5.1 — the report fails closed.
///
/// §3 found that the foreign symbol list is a proof about names and a
/// heuristic about domains, and left the report saying `ffi` as calmly as
/// it says `heap`. A supervisor that reads the row and trusts it would
/// then admit `examples/serve/` under `network: None`.
///
/// So the report says it first, in a field a naive consumer cannot miss:
/// `bounded` is the first key, and it is `false` whenever any reachable
/// label fails to name its own domain -- which is exactly `ffi`. Refusing
/// on `bounded: false` is the safe default; trusting a particular
/// unbounded program anyway is a decision about that program, and it is
/// the supervisor's to make rather than the report's.
///
/// Both directions, because a flag that is always `false` would pass the
/// first half of this test and be worthless.
#[test]
fn the_report_fails_closed() {
    let report = |relative: &str| -> String {
        let out = Command::new(BIN)
            .args(["authority".as_ref(), repo_root().join(relative).as_os_str()])
            .args(["--std", "--output", "json"])
            .output()
            .expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("the report is utf-8")
    };

    let serve = report("examples/serve/serve.ls");
    assert!(
        serve.trim_start().starts_with("{\n  \"bounded\": false,"),
        "a program reaching foreign code must lead with `bounded: false`:\n{serve}"
    );

    let cut = report("examples/cut/cut.ls");
    assert!(
        cut.trim_start().starts_with("{\n  \"bounded\": true,"),
        "a program with no foreign code is bounded, or the flag means nothing:\n{cut}"
    );

    // And the prose form says so before anything else.
    let prose = Command::new(BIN)
        .args(["authority".as_ref(), repo_root().join("examples/serve/serve.ls").as_os_str()])
        .arg("--std")
        .output()
        .expect("the compiler runs");
    let prose = String::from_utf8_lossy(&prose.stdout);
    assert!(prose.starts_with("UNBOUNDED"), "the prose report should open with it:\n{prose}");
    assert!(prose.contains("ffi(\"libc\")    <- unbounded"), "and mark the label:\n{prose}");
}

/// `docs/under-a-grant.md` §3 — the symbol list is a proof about *names*.
///
/// `reach.md` §5.2 said a supervisor reading `socket`, `bind`, `listen`
/// and `accept` knows what it is being asked to run "without trusting a
/// word the program says about itself". The first half is right. The
/// second is not: every foreign call needs a declaration, and the
/// *declaration* is the program's to name.
///
/// Six lines open a socket and report `["syscall"]`. Not a hole to be
/// plugged — refusing this one name would be theatre, since the next
/// spelling is a wrapper. The hole is `Ffi(lib)`'s width (§4).
#[test]
fn the_symbol_list_is_a_proof_about_names() {
    let dir = scratch("under-a-grant");
    let source = dir.join("opaque.ls");
    std::fs::write(
        &source,
        "extern fn syscall[&f](ffi: &f Ffi(\"libc\"), n: int, a: int, b: int, c: int)\n\
             -> [ffi(\"libc\")] int;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(io); release(fs); release(heap); release(args);\n\
             var r = 0;\n\
             let libc = narrow(ffi, \"libc\");\n\
             borrow libc as &f in { r = syscall(f, 41, 2, 1, 0); }\n\
             release(libc);\n\
             if r < 0 { return 1; }\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");

    let out = Command::new(BIN)
        .args(["authority".as_ref(), source.as_os_str(), "--output".as_ref(), "json".as_ref()])
        .output()
        .expect("the compiler runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report = String::from_utf8_lossy(&out.stdout);

    assert!(
        report.contains("\"effects\": [\"ffi\"]"),
        "a program that opens a socket through `syscall` reports only `ffi`:\n{report}"
    );
    assert!(
        report.contains("\"foreign_symbols\": [\"syscall\"]"),
        "and its symbol list names nothing a supervisor could map to a domain:\n{report}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/under-a-grant.md` §2 and §4 — the dimension that *does* work.
///
/// `filesystem.md` §2 took files out of libc and made them builtins under
/// `Fs(p)`, so the path survives into the report. That is the whole reason
/// the grant's filesystem dimension is enforceable while its network and
/// exec dimensions are not — someone already did, for files, what §5 asks
/// for sockets.
#[test]
fn the_filesystem_dimension_is_enforceable() {
    let (_, _, labels) = authority_of("tests/accept/fs_narrowed.ls");
    assert!(
        labels.iter().any(|label| label.starts_with("fs_write=/")),
        "the report should carry the path prefix, not just the dimension: {labels:?}"
    );
}
