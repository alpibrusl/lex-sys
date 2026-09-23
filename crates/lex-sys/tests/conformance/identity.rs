//! Content hashes and canonical printing: what moves an identity and what does not.

use super::*;

/// The canonical printer's two contracts, over every `.ls` file in the repo.
///
/// 1. **Identity-preserving.** Parsing the printed text gives back the same
///    hash for every declaration. That is what makes it the rendering step
///    of a store that addresses code by hash rather than merely a
///    pretty-printer.
/// 2. **Idempotent.** Printing the output again changes nothing, so the
///    canonical form is a fixed point.
///
/// Run over the accept fixtures, the examples *and* the reject fixtures --
/// the last of those parse even though they are refused later, and they are
/// where the odd syntax lives, so they are the most valuable input of the
/// three.
#[test]
fn printing_preserves_every_identity_and_is_idempotent() {
    let mut checked = 0;
    // `examples/wordfreq` is listed separately because the walkers here
    // filter on the `.ls` extension, which a directory does not have --
    // that is what keeps a multi-file example out of the single-file
    // harnesses, and it would keep it out of this one too.
    for dir in [
        "tests/accept",
        "tests/reject",
        "examples",
        "examples/wordfreq",
        "examples/buffer",
        "examples/slab",
        "examples/modular",
        "examples/serve",
        "examples/fetch",
        "examples/report",
        // The benchmarks are code too, and the pairs are the place a
        // careless edit would land without anyone reading it.
        "benches",
        "examples/base64",
        "examples/sort",
        "benches/three",
        // The standard library is code, and gets the same contract every
        // other file here gets: printed, reparsed, identical hashes, and
        // a fixed point.
        "std",
    ] {
        for entry in std::fs::read_dir(repo_root().join(dir)).expect("a readable directory") {
            let path = entry.expect("a readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("ls") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a readable fixture");
            // A reject fixture may be refused by the *parser*, in which case
            // there is no tree to print and nothing to check here.
            let Ok(ast) = lex_sys_syntax::parse(&source) else { continue };

            let printed = lex_sys_syntax::print(&ast);
            let reparsed = lex_sys_syntax::parse(&printed).unwrap_or_else(|d| {
                panic!(
                    "{}: printed output does not parse: {}\n{printed}",
                    path.display(),
                    d.message
                )
            });

            let before = lex_sys_id::identify(&ast);
            let after = lex_sys_id::identify(&reparsed);
            assert_eq!(
                before.functions.len(),
                after.functions.len(),
                "{}: a declaration went missing",
                path.display()
            );
            for (a, b) in before.functions.iter().zip(after.functions.iter()) {
                assert_eq!(a.sig, b.sig, "{}: `{}`'s signature changed", path.display(), a.name);
                assert_eq!(a.body, b.body, "{}: `{}`'s body changed", path.display(), a.name);
            }
            for (a, b) in before.types.iter().zip(after.types.iter()) {
                assert_eq!(a.id, b.id, "{}: `{}` changed", path.display(), a.name);
            }

            let again = lex_sys_syntax::print(&reparsed);
            assert_eq!(printed, again, "{}: printing is not a fixed point", path.display());
            checked += 1;
        }
    }
    assert!(checked > 100, "the walk should have found the whole suite, found {checked}");
}

/// A module's whole cost to the identity system, which is nothing
/// (`docs/modules.md` §2).
///
/// `canonical-ast.md` §1 has said since M0 that "moving a function
/// between files changes nothing about it". A module could have broken
/// that, and did not -- because a call already encodes the callee's
/// **hash** rather than its spelling, for an unrelated reason.
///
/// So this compiles the same two functions twice: once flat, once with
/// the callee in a module and the caller reaching it through an import.
/// All four hashes must be identical. Not similar -- the same.
#[test]
fn moving_a_function_into_a_module_changes_no_hash() {
    let ids = |tag: &str, files: &[(&str, &str)]| -> String {
        let dir = scratch(tag);
        let mut command = Command::new(BIN);
        command.arg("ids");
        for (name, source) in files {
            let path = dir.join(name);
            std::fs::write(&path, source).expect("a writable fixture");
            command.arg(path);
        }
        let out = command.output().expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        // Sorted, because the two programs list their declarations in
        // different orders and this is a claim about hashes, not order.
        let mut lines: Vec<String> =
            String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect();
        lines.sort();
        let _ = std::fs::remove_dir_all(&dir);
        lines.join("\n")
    };

    let flat = ids(
        "modules-identity-flat",
        &[(
            "flat.ls",
            "fn twice(n: int) -> [] int { return n + n; }\n\
             fn caller() -> [] int { return twice(21); }\n",
        )],
    );
    let modular = ids(
        "modules-identity-modular",
        &[
            ("user.ls", "import m;\nfn caller() -> [] int { return m.twice(21); }\n"),
            ("lib.ls", "module m;\npub fn twice(n: int) -> [] int { return n + n; }\n"),
        ],
    );

    assert_eq!(flat, modular, "a module reached the hash, and it must not");
}

#[test]
fn identity_is_content_not_location() {
    // §3, and the reason that section exists. `canonical-ast.md` §1 has
    // claimed since M0 that "moving a function between files changes
    // nothing about it". With one file there were no files to move
    // between; with several there are, so it is checked.
    //
    // The same function, in two programs, at different positions, with
    // different neighbours, in differently named files: same `SigId`,
    // same `BodyId`.
    let dir = scratch("many-files-identity");
    let alone = dir.join("alone.ls");
    let crowded = dir.join("crowded.ls");
    let body = "fn double(n: int) -> [] int { return n + n; }\n";
    let main = "fn main(world: World) -> [] int {\n\
                    let Split { io, ffi, fs, heap, args } = split(world);\n\
                    release(args); release(heap); release(fs); release(ffi); release(io);\n\
                    return double(0);\n\
                }\n";
    std::fs::write(&alone, format!("{body}{main}")).expect("a writable fixture");
    std::fs::write(
        &crowded,
        format!("fn unrelated(n: int) -> [] int {{ return n * 3; }}\n{body}{main}"),
    )
    .expect("a writable fixture");

    let ids_of = |path: &Path| -> String {
        let out = Command::new(BIN).arg("ids").arg(path).output().expect("the compiler runs");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("double"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let first = ids_of(&alone);
    assert!(!first.is_empty(), "`double` should have hashes");
    assert_eq!(first, ids_of(&crowded), "a unit hashes its content, not where it sits");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/tuples.md` §5: a tuple is a struct with the names removed, so
/// replacing one with the other changes no generated code.
///
/// Stated that way it is a claim about a compiler, and the strongest form
/// of it available is the one asserted here: the two programs below differ
/// only in whether the pair is a declared `res struct` or a tuple, and
/// their **object files are byte-identical**. Not similar, not the same
/// size -- the same bytes.
///
/// That is what makes tuples an ergonomic feature rather than a
/// representation choice, and it is why `examples/slab/` could drop two
/// declared types without anyone having to ask what it cost.
#[test]
fn a_tuple_emits_the_same_object_as_the_struct_it_replaces() {
    const STRUCT: &str = "\
res struct Pair { held: Box[int], tag: int }
fn make[&h](heap: &!h Heap, n: int) -> [heap] Pair {
    return Pair { held: box(heap, n), tag: n + 1 };
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(ffi); release(fs); release(io);
    var status = 0;
    borrow mut heap as &!h in {
        let p = make(h, 41);
        let Pair { held, tag } = p;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status - 83;
}
";
    const TUPLE: &str = "\
fn make[&h](heap: &!h Heap, n: int) -> [heap] (Box[int], int) {
    return (box(heap, n), n + 1);
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(ffi); release(fs); release(io);
    var status = 0;
    borrow mut heap as &!h in {
        let p = make(h, 41);
        let (held, tag) = p;
        status = unbox(h, held) + tag;
    }
    release(heap);
    return status - 83;
}
";

    let dir = scratch("tuple-layout");
    let mut objects = Vec::new();
    for (name, source) in [("declared", STRUCT), ("anonymous", TUPLE)] {
        let path = dir.join(format!("{name}.ls"));
        std::fs::write(&path, source).expect("a writable fixture");
        let object = dir.join(format!("{name}.o"));
        let build = Command::new(BIN)
            .args([
                "build".as_ref(),
                path.as_os_str(),
                "--emit".as_ref(),
                "obj".as_ref(),
                "-o".as_ref(),
                object.as_os_str(),
            ])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        objects.push(std::fs::read(&object).expect("a readable object file"));
    }

    assert_eq!(
        objects[0], objects[1],
        "a tuple and the struct it replaces must emit the same object file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ids_are_stable_across_runs_and_survive_a_body_rewrite() {
    let source = repo_root().join("examples").join("rational.ls");

    let once = Command::new(BIN).arg("ids").arg(&source).output().expect("the compiler runs");
    assert!(once.status.success(), "{}", String::from_utf8_lossy(&once.stderr));
    let again = Command::new(BIN).arg("ids").arg(&source).output().expect("the compiler runs");
    assert_eq!(once.stdout, again.stdout, "hashing is a function of the program alone");

    let text = String::from_utf8(once.stdout).expect("hashes are ascii");
    assert!(text.contains("sig  harmonic"), "{text}");
    assert!(text.contains("type Rational"), "{text}");

    // Rewrite a body without touching any signature: every `sig` line must be
    // unchanged and at least one `body` line must move.
    let dir = scratch("ids-rewrite");
    let rewritten = dir.join("rational.ls");
    let original = std::fs::read_to_string(&source).expect("a readable example");
    let patched = original.replace(
        "fn abs(x: int) -> [] int {\n    if x < 0 {\n        return 0 - x;\n    }\n    return x;\n}",
        "fn abs(x: int) -> [] int {\n    if x >= 0 {\n        return x;\n    }\n    return 0 - x;\n}",
    );
    assert_ne!(patched, original, "the body rewrite should have applied");
    std::fs::write(&rewritten, patched).expect("a writable copy");

    let after = Command::new(BIN).arg("ids").arg(&rewritten).output().expect("the compiler runs");
    assert!(after.status.success(), "{}", String::from_utf8_lossy(&after.stderr));
    let after = String::from_utf8(after.stdout).expect("hashes are ascii");

    let sigs = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("sig ") || l.starts_with("type "))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(sigs(&text), sigs(&after), "a body rewrite must not move any signature");
    assert_ne!(text, after, "it must move a body");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/bitwise.md` §6 — the operator set grew and no hash moved.
///
/// An operator is hashed by a code inside `Binary`/`Unary` rather than by
/// a node tag of its own, and the new codes were *appended*. So a program
/// written before this slice hashes to what it hashed before.
///
/// The values below were not recomputed after the change. They were taken
/// from a build of `83cc7a7`, the commit immediately before this slice, and
/// the two compilers print the same bytes for the same source — which is
/// what makes this a check rather than a restatement.
#[test]
fn ids_are_stable_across_the_operator_set() {
    // Deliberately uses only the operators that existed beforehand.
    let source = "fn mix(a: int, b: int) -> [] bool {\n\
                  \x20   return a + b * 2 - 1 < 10 && !(a == b);\n\
                  }\n";
    let scratch = scratch("ids-operator-set");
    let path = scratch.join("mix.ls");
    std::fs::write(&path, source).expect("a writable fixture");

    let output = Command::new(BIN).arg("ids").arg(&path).output().expect("the compiler runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).expect("hashes are ascii");

    // The whole point is that these are *literal* and were not recomputed
    // after the operators landed. If a future slice inserts an operator
    // code rather than appending one, this is what says so.
    assert!(
        text.contains("3c635be9a28ee7ea3f1f096db9c25f312d6ffd5f43c89e58820361e2e01f25fc"),
        "a signature moved; §6's append-only rule was broken\n{text}"
    );
    assert!(
        text.contains("d8f04b00ae8eef0ca032ba48f781d453a9f283116b410bd20c03a835f73fd1ad"),
        "a body moved; §6's append-only rule was broken\n{text}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// `docs/character-literals.md` §5 — the third spelling moved no hash.
///
/// `'a'` is the integer 97 and nothing past `int_value` knows which
/// spelling was written, so the two programs below are one program. This
/// is the same claim `ids_are_stable_across_the_operator_set` makes for
/// hexadecimal, checked the way `bitwise.md` §1.1 says it should be:
/// against the other spelling rather than against a number written down.
///
/// Checking it against its sibling rather than against a literal hash is
/// deliberate. A frozen hash here would fail on any future encoder
/// change, including a correct one, and say nothing about the property
/// this slice is responsible for — which is that *these two texts agree*,
/// whatever they agree on.
#[test]
fn a_character_literal_hashes_as_its_integer() {
    let scratch = scratch("ids-character-literal");
    let spellings = [
        ("numbers", "fn f(c: int) -> [] int { return c + 48 + 10; }\n"),
        ("characters", "fn f(c: int) -> [] int { return c + '0' + '\\n'; }\n"),
    ];

    let mut hashes = Vec::new();
    for (name, source) in spellings {
        let path = scratch.join(format!("{name}.ls"));
        std::fs::write(&path, source).expect("a writable fixture");
        let output = Command::new(BIN).arg("ids").arg(&path).output().expect("the compiler runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        hashes.push(String::from_utf8(output.stdout).expect("hashes are ascii"));
    }

    assert_eq!(
        hashes[0], hashes[1],
        "`'0'` and `48` should be one node, so these should be one program"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
