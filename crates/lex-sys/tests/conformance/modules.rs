//! Programs over several files and modules, and the standard library behind `--std`.

use super::*;

/// Build a program from several named files, in the order given.
fn build_many(tag: &str, files: &[(&str, &str)]) -> (PathBuf, std::process::Output) {
    let dir = scratch(tag);
    let mut paths: Vec<PathBuf> = Vec::new();
    for (name, source) in files {
        let path = dir.join(name);
        std::fs::write(&path, source).expect("a writable fixture");
        paths.push(path);
    }
    let exe = dir.join("program");
    let mut command = Command::new(BIN);
    command.arg("build");
    for path in &paths {
        command.arg(path);
    }
    command.arg("-o").arg(&exe);
    let build = command.output().expect("the compiler runs");
    (exe, build)
}

const UTIL_LS: &str = "fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int {\n\
                           if n >= 10 { print_nat(io, n / 10); }\n\
                           return putchar(io, 48 + n % 10);\n\
                       }\n";

/// The multi-file half of `docs/modules.md` §8's suite.
///
/// These need two files each, so they cannot be `tests/reject/` fixtures
/// -- that walker compiles one file at a time. Same discipline all the
/// same: every rule in the document has a program that breaks it, and the
/// message it is refused with is written down.
#[test]
fn the_module_rules_are_enforced_across_files() {
    const LIB: &str = "module lib;\n\
                       pub fn shown() -> [] int { return 1; }\n\
                       fn hidden() -> [] int { return 2; }\n\
                       struct Secret { n: int }\n\
                       pub struct Open { n: int }\n";

    let refused = |tag: &str, main: &str| -> String {
        let (_, build) = build_many(tag, &[("main.ls", main), ("lib.ls", LIB)]);
        assert!(!build.status.success(), "`{tag}` should have been refused");
        String::from_utf8_lossy(&build.stderr).into_owned()
    };

    // §5: private is private.
    let private = refused(
        "modules-private",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return lib.hidden();\n\
         }\n",
    );
    assert!(private.contains("`hidden` is not `pub`"), "{private}");

    // §5, for a type rather than a function.
    let private_type = refused(
        "modules-private-type",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             let s: lib.Secret = lib.Secret { n: 1 };\n\
             return s.n;\n\
         }\n",
    );
    assert!(private_type.contains("`Secret` is not `pub`"), "{private_type}");

    // §4.1: an import binds a qualifier, not a set of names. `shown` is
    // `pub` and imported, and still not in scope unqualified.
    let unqualified = refused(
        "modules-unqualified",
        "import lib;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return shown();\n\
         }\n",
    );
    assert!(unqualified.contains("`shown` is not a function"), "{unqualified}");

    // §4: a qualified name that is not there is an error, never a
    // fall back to the local module. `elsewhere` is defined right here.
    let missing = refused(
        "modules-missing",
        "import lib;\n\
         fn elsewhere() -> [] int { return 3; }\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap); release(io);\n\
             return lib.elsewhere();\n\
         }\n",
    );
    assert!(missing.contains("`elsewhere` is not a function"), "{missing}");

    // §4: two imports may not bind one qualifier.
    let (_, collision) = build_many(
        "modules-collision",
        &[
            (
                "main.ls",
                "import lib;\n\
                 import other.lib;\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return 0;\n\
                 }\n",
            ),
            ("lib.ls", LIB),
            ("other.ls", "module other.lib;\npub fn nothing() -> [] int { return 0; }\n"),
        ],
    );
    assert!(!collision.status.success(), "two imports bound `lib`");
    let text = String::from_utf8_lossy(&collision.stderr);
    assert!(text.contains("is already bound to another import"), "{text}");

    // And the same program with an `as` is accepted, which is what makes
    // the refusal above a rule rather than a limit.
    let (_, renamed) = build_many(
        "modules-renamed",
        &[
            (
                "main.ls",
                "import lib;\n\
                 import other.lib as other;\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return lib.shown() + other.nothing() - 1;\n\
                 }\n",
            ),
            ("lib.ls", LIB),
            ("other.ls", "module other.lib;\npub fn nothing() -> [] int { return 0; }\n"),
        ],
    );
    assert!(renamed.status.success(), "{}", String::from_utf8_lossy(&renamed.stderr));
}

/// `docs/collections.md` §5: a `match` names an enum through the same
/// qualifier every other reference uses.
///
/// This is not decoration. `Pattern::Variant` carried no qualifier, so a
/// `match` could only name an enum its own module declared — which makes
/// an imported enum a type a program can hold, pass around and **never
/// take apart**. `std.option` is unusable without this, and so is every
/// enum any library will ever export.
#[test]
fn a_match_names_an_enum_through_its_qualifier() {
    const SHAPES: &str = "module shapes;\n\
                          pub enum Shape { Flat, Tall(int) }\n\
                          pub fn tall(n: int) -> [] Shape { return Shape::Tall(n); }\n";

    let (exe, build) = build_many(
        "modules-qualified-pattern",
        &[
            (
                "main.ls",
                "import shapes;\n\
                 fn height(s: shapes.Shape) -> [] int {\n\
                     match s {\n\
                         shapes.Shape::Flat => { return 0; }\n\
                         shapes.Shape::Tall(n) => { return n; }\n\
                     }\n\
                 }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return height(shapes.tall(7)) + height(shapes.Shape::Flat) - 7;\n\
                 }\n",
            ),
            ("shapes.ls", SHAPES),
        ],
    );
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0));

    // And the qualifier is checked rather than decorative: a name that is
    // not an import here is an error, not something skipped over because
    // the scrutinee already said which enum this is.
    let (_, wrong) = build_many(
        "modules-qualified-pattern-unbound",
        &[
            (
                "main.ls",
                "import shapes;\n\
                 fn height(s: shapes.Shape) -> [] int {\n\
                     match s {\n\
                         forms.Shape::Flat => { return 0; }\n\
                         forms.Shape::Tall(n) => { return n; }\n\
                     }\n\
                 }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(ffi); release(fs); release(heap); release(io);\n\
                     return height(shapes.Shape::Flat);\n\
                 }\n",
            ),
            ("shapes.ls", SHAPES),
        ],
    );
    assert!(!wrong.status.success(), "`forms` is not imported");
    let text = String::from_utf8_lossy(&wrong.stderr);
    assert!(text.contains("`forms` is not an imported module here"), "{text}");
}

/// The standard library type-checks on its own, with no program
/// (`docs/standard-library.md` §7).
///
/// Named on the command line like any other module -- which is the point
/// of `modules.md`: the library is not special, it is just files whose
/// source happens to ship in the compiler.
#[test]
fn the_standard_library_compiles_on_its_own() {
    let root = repo_root().join("std");
    let mut command = Command::new(BIN);
    command.arg("check");
    for name in
        ["bytes.ls", "math.ls", "io.ls", "buffer.ls", "option.ls", "result.ls", "list.ls", "vec.ls"]
    {
        command.arg(root.join(name));
    }
    let out = command.output().expect("the compiler runs");
    // No `main`, so the CLI refuses at the end -- but only after every
    // declaration has been checked, which is what this is asserting.
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("no `main` function"),
        "the library itself should check clean; got:\n{text}"
    );
}

/// `--std` makes the library's source present without naming a file
/// (§2), and it is still opt-in: the program writes its own `import`.
#[test]
fn std_is_available_behind_a_flag() {
    let dir = scratch("std-flag");
    let source = dir.join("tool.ls");
    std::fs::write(
        &source,
        "import std.io;\n\
         import std.bytes;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(ffi); release(fs); release(heap);\n\
             borrow mut io as &!i in {\n\
                 io.print_pad(i, 0 - 42, 6);\n\
                 io.newline(i);\n\
             }\n\
             release(io);\n\
             return bytes.digit_of(55) - 7;\n\
         }\n",
    )
    .expect("a writable fixture");
    let exe = dir.join("tool");
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
    assert_eq!(String::from_utf8_lossy(&run.stdout), "   -42\n");
    assert_eq!(run.status.code(), Some(0), "`digit_of('7')` is 7");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/standard-library.md` §5.2: a declaration nobody calls costs
/// nothing.
///
/// The same program built with `--std` and without it emits
/// **byte-identical** object files. Not smaller-by-a-bit -- the same
/// bytes, because emission is driven by what `main` reaches and an
/// unreached declaration is still checked and never lowered.
///
/// This claim was **false** when it was first written down, which is why
/// it is a test: the library added 5.6 KB to a program that called none
/// of it, because pass 2 seeded from every non-generic function rather
/// than from the entry point.
#[test]
fn std_declarations_cost_nothing_unless_called() {
    const BARE: &str = "fn main(world: World) -> [] int {\n\
                            let Split { io, ffi, fs, heap, args } = split(world);\n\
                            release(args); release(ffi); release(fs); release(heap);\n\
                            borrow mut io as &!i in { putchar(i, 65); }\n\
                            release(io);\n\
                            return 0;\n\
                        }\n";
    let dir = scratch("std-costs-nothing");
    let source = dir.join("bare.ls");
    std::fs::write(&source, BARE).expect("a writable fixture");

    let object = |name: &str, extra: &[&str]| -> Vec<u8> {
        let out = dir.join(name);
        let mut command = Command::new(BIN);
        command.arg("build").arg(&source);
        for flag in extra {
            command.arg(flag);
        }
        command.arg("--emit").arg("obj").arg("-o").arg(&out);
        let build = command.output().expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        std::fs::read(&out).expect("a readable object file")
    };

    assert_eq!(
        object("without.o", &[]),
        object("with.o", &["--std"]),
        "the standard library reached the output of a program that never calls it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_can_be_spread_over_several_files() {
    // `docs/many-files.md` §2: a program is a set of files, named on the
    // command line in any order, sharing one flat namespace.
    //
    // `main.ls` calls `print_nat`, which is declared in a file listed
    // *after* it, and `twice`, declared in a third. Order does not matter
    // because there is no order to matter: the files are one program.
    let (exe, build) = build_many(
        "many-files",
        &[
            (
                "main.ls",
                "fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi);\n\
                     borrow mut io as &!i in { print_nat(i, twice(21)); putchar(i, 10); }\n\
                     release(io);\n\
                     return twice(21) - 42;\n\
                 }\n",
            ),
            ("util.ls", UTIL_LS),
            ("math.ls", "fn twice(n: int) -> [] int { return n + n; }\n"),
        ],
    );
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert_eq!(run.status.code(), Some(0));

    let _ = std::fs::remove_dir_all(exe.parent().expect("a directory"));
}

#[test]
fn a_diagnostic_names_the_file_it_came_from() {
    // §4: spans are offsets into the whole program's source, and a
    // `SourceMap` resolves one back to a file, a line and a column. The
    // error here is in the *third* file, several thousand bytes into the
    // program, and has to be reported at that file's own line 1.
    let dir = scratch("many-files-diagnostic");
    let main = dir.join("main.ls");
    let util = dir.join("util.ls");
    let broken = dir.join("broken.ls");
    std::fs::write(
        &main,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world);\n\
             release(args); release(heap); release(fs); release(ffi); release(io);\n\
             return 0;\n\
         }\n",
    )
    .expect("a writable fixture");
    std::fs::write(&util, UTIL_LS).expect("a writable fixture");
    std::fs::write(&broken, "fn oops() -> [] int { return missing(); }\n")
        .expect("a writable fixture");

    let output = Command::new(BIN)
        .args(["check".as_ref(), main.as_os_str(), util.as_os_str(), broken.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!("{}:1:", broken.display())), "{stderr}");
    assert!(stderr.contains("`missing` is not a function"), "{stderr}");
    // The offending source line, from the right file.
    assert!(stderr.contains("fn oops() -> [] int"), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_is_declared_once_per_program_not_per_file() {
    // §2.2: the namespace is flat and shared, so a duplicate across two
    // files is the same error as a duplicate within one. Nothing new had
    // to be invented -- this is `duplicate_function.ls` noticing a second
    // file.
    let (_, build) = build_many(
        "many-files-duplicate",
        &[
            (
                "main.ls",
                "fn twice(n: int) -> [] int { return n + n; }\n\
                 fn main(world: World) -> [] int {\n\
                     let Split { io, ffi, fs, heap, args } = split(world);\n\
                     release(args); release(heap); release(fs); release(ffi); release(io);\n\
                     return twice(0);\n\
                 }\n",
            ),
            ("other.ls", "fn twice(n: int) -> [] int { return n * 2; }\n"),
        ],
    );
    assert_eq!(build.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(stderr.contains("twice"), "{stderr}");

    let _ = std::fs::remove_dir_all(scratch("many-files-duplicate"));
}
