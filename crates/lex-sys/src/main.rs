//! The lex-sys bootstrap compiler CLI.
//!
//! The v0 compiler is hosted in Rust (an open decision in #1, settled here and
//! recorded in `docs/bootstrap.md`). It compiles one file at a time: M0 has no
//! module system, and a unit is a file.
//!
//! Exit codes are semantic, as they are across the rest of the ecosystem:
//!
//! | code | meaning |
//! |---|---|
//! | 0 | success |
//! | 1 | the program was refused (a located diagnostic was printed) |
//! | 2 | the command line was wrong |
//! | 3 | the environment failed: no linker, unwritable output, unsupported host |
//!
//! `run` is the exception: it replaces its own status with the compiled
//! program's, so `lex-sys run p.ls` and `lex-sys build p.ls && ./p` agree.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use lex_sys_syntax::{Ast, SourceFile, SourceMap};

const USAGE: &str = "\
lex-sys — the bootstrap compiler for the lex-sys systems dialect

usage:
    lex-sys build <file.ls>... [-o <output>] [--emit exe|obj] [--std]
    lex-sys check <file.ls>... [--std]
    lex-sys run   <file.ls>... [--std]
    lex-sys ids   <file.ls>... [--std]
    lex-sys print <file.ls>
    lex-sys --version

options:
    -o <output>     where to write the result (default: the first input's stem)
    --emit exe|obj  emit a linked executable (default) or a bare object file
    --std           make the standard library's source available

A program is the set of files named on the command line, in any order.
Each file is in a module -- the root, unless it says `module a.b;` -- and
reaches another module's names through `import`. See docs/many-files.md
and docs/modules.md.

`--std` adds the standard library's source, which is compiled into this
binary rather than looked up on disk: no search path, no manifest. It is
not a prelude -- a program still writes `import std.io;` where it uses
one -- and a declaration nothing calls emits nothing. See
docs/standard-library.md.

`ids` prints each declaration's content hash: a signature and a body for
every function, one identity for every type. A unit hashes its content,
not the file it sits in. See docs/canonical-ast.md.

`print` renders one parsed file in canonical form. It is the AST-to-text
direction of that same pipeline, not a formatter: comments never reach the
AST, so they are not in the output.
";

const EXIT_REFUSED: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_ENVIRONMENT: u8 = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(Failure { message, code }) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}

struct Failure {
    message: String,
    code: u8,
}

fn refused(message: impl Into<String>) -> Failure {
    Failure { message: message.into(), code: EXIT_REFUSED }
}

fn usage(message: impl Into<String>) -> Failure {
    Failure { message: format!("{}\n\n{USAGE}", message.into()), code: EXIT_USAGE }
}

fn environment(message: impl Into<String>) -> Failure {
    Failure { message: message.into(), code: EXIT_ENVIRONMENT }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Emit {
    Exe,
    Obj,
}

fn run(args: &[String]) -> Result<ExitCode, Failure> {
    let Some(command) = args.first() else {
        return Err(usage("no command given"));
    };

    match command.as_str() {
        "--version" | "-V" => {
            println!(
                "lex-sys {} (host {})",
                env!("CARGO_PKG_VERSION"),
                lex_sys_codegen::host_triple()
            );
            Ok(ExitCode::SUCCESS)
        }
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let (inputs, _, _, with_std) = parse_args(&args[1..], false)?;
            compile_to_ir(&inputs, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        // `docs/many-files.md` §5: printing is about text, and text is
        // what a file is -- so this renders exactly one.
        "print" => {
            let (inputs, _, _, _) = parse_args(&args[1..], false)?;
            let [input] = &inputs[..] else {
                return Err(usage("`print` renders one file at a time"));
            };
            let text = std::fs::read_to_string(input)
                .map_err(|e| environment(format!("cannot read `{}`: {e}", input.display())))?;
            let file = SourceFile::new(input.display().to_string(), text);
            let ast = lex_sys_syntax::parse(&file.text).map_err(|d| refused(d.render(&file)))?;
            print!("{}", lex_sys_syntax::print(&ast));
            Ok(ExitCode::SUCCESS)
        }
        "ids" => {
            let (inputs, _, _, with_std) = parse_args(&args[1..], false)?;
            print_ids(&inputs, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        "build" => {
            let (inputs, output, emit, with_std) = parse_args(&args[1..], true)?;
            let output = output.unwrap_or_else(|| default_output(&inputs[0], emit));
            build(&inputs, &output, emit, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        "run" => {
            let (inputs, _, _, with_std) = parse_args(&args[1..], false)?;
            let dir = std::env::temp_dir().join(format!("lex-sys-run-{}", std::process::id()));
            std::fs::create_dir_all(&dir)
                .map_err(|e| environment(format!("cannot create `{}`: {e}", dir.display())))?;
            let exe =
                dir.join(default_output(&inputs[0], Emit::Exe).file_name().unwrap_or_default());
            let result = build(&inputs, &exe, Emit::Exe, with_std).and_then(|()| {
                Command::new(&exe)
                    .status()
                    .map_err(|e| environment(format!("cannot run `{}`: {e}", exe.display())))
            });
            let _ = std::fs::remove_dir_all(&dir);
            let status = result?;
            Ok(ExitCode::from(status.code().unwrap_or(EXIT_ENVIRONMENT as i32) as u8))
        }
        other => Err(usage(format!("unknown command `{other}`"))),
    }
}

/// The standard library's source, compiled into this binary
/// (`docs/standard-library.md` §2).
///
/// Embedded rather than looked up, so `--std` adds **no search path**,
/// no manifest and no build step -- `modules.md` §6 promised none of
/// those, and this keeps the promise by never going near the disk.
///
/// The cost is that the library's version is the compiler's version.
/// For a language at this stage that is the right trade -- one artifact,
/// one thing to install, nothing to resolve -- and §2.1 records it as
/// the first thing to revisit when a package story exists.
const STD: &[(&str, &str)] = &[
    ("<std>/bytes.ls", include_str!("../../../std/bytes.ls")),
    ("<std>/math.ls", include_str!("../../../std/math.ls")),
    ("<std>/io.ls", include_str!("../../../std/io.ls")),
    ("<std>/buffer.ls", include_str!("../../../std/buffer.ls")),
    ("<std>/option.ls", include_str!("../../../std/option.ls")),
    ("<std>/result.ls", include_str!("../../../std/result.ls")),
    ("<std>/list.ls", include_str!("../../../std/list.ls")),
    ("<std>/vec.ls", include_str!("../../../std/vec.ls")),
];

fn parse_args(
    args: &[String],
    allow_output: bool,
) -> Result<(Vec<PathBuf>, Option<PathBuf>, Emit, bool), Failure> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut output = None;
    let mut emit = Emit::Exe;
    let mut with_std = false;
    let mut it = args.iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" if allow_output => {
                let value = it.next().ok_or_else(|| usage("`-o` needs a path"))?;
                output = Some(PathBuf::from(value));
            }
            "--emit" if allow_output => {
                emit = match it.next().map(String::as_str) {
                    Some("exe") => Emit::Exe,
                    Some("obj") => Emit::Obj,
                    Some(other) => return Err(usage(format!("unknown emit kind `{other}`"))),
                    None => return Err(usage("`--emit` needs a kind")),
                };
            }
            // `docs/standard-library.md` §2. Opt-in, and never an
            // implicit prelude: this decides whether the library's
            // *source* is present, never whether a name is in scope. A
            // program still writes `import std.io;` where it uses it.
            "--std" => with_std = true,
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option `{other}`")));
            }
            // `docs/many-files.md` §2: a program is a set of files, named
            // on the command line in any order.
            other => inputs.push(PathBuf::from(other)),
        }
    }

    if inputs.is_empty() {
        return Err(usage("no input file given"));
    }
    Ok((inputs, output, emit, with_std))
}

fn default_output(input: &Path, emit: Emit) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default();
    match emit {
        Emit::Exe => PathBuf::from(stem),
        Emit::Obj => PathBuf::from(format!("{}.o", stem.to_string_lossy())),
    }
}

/// Read and parse every file of a program into one AST
/// (`docs/many-files.md` §2).
///
/// The `SourceMap` hands out a base offset per file and resolves any
/// diagnostic's span back to the file it came from, so nothing downstream
/// of here learns that a program can have more than one (§4).
fn parse_program(inputs: &[PathBuf], with_std: bool) -> Result<(Ast, SourceMap), Failure> {
    let mut map = SourceMap::new();
    let mut ast = Ast::new();
    let mut sources = Vec::new();
    for input in inputs {
        let text = std::fs::read_to_string(input)
            .map_err(|e| environment(format!("cannot read `{}`: {e}", input.display())))?;
        let base = map.add(input.display().to_string(), text.clone());
        sources.push((text, base));
    }
    // The library is more files of the same program (`many-files.md` §2)
    // -- it arrives from `include_str!` rather than from the disk, and
    // nothing downstream of here can tell the difference. Named
    // `<std>/io.ls` in the map, so a diagnostic inside the library says
    // so rather than naming a path that does not exist.
    if with_std {
        for (name, text) in STD {
            let base = map.add((*name).to_owned(), (*text).to_owned());
            sources.push(((*text).to_owned(), base));
        }
    }
    for (text, base) in &sources {
        lex_sys_syntax::parse_into(&mut ast, text, *base)
            .map_err(|d| refused(d.render_in(&map)))?;
    }
    Ok((ast, map))
}

/// Read, parse and lower a program, reporting any refusal with its source line.
fn compile_to_ir(inputs: &[PathBuf], with_std: bool) -> Result<lex_sys_ir::Program, Failure> {
    let (ast, map) = parse_program(inputs, with_std)?;
    let program = lex_sys_ir::lower(&ast).map_err(|d| refused(d.render_in(&map)))?;
    let where_ = inputs[0].display().to_string();

    // `main` becomes the process entry point, so its shape is part of the
    // contract with the C runtime rather than a matter of taste.
    if let Some(entry) = program.find("main") {
        let entry = program.func(entry);
        // §8.2: the runtime hands over exactly one `World`, and it is the
        // only place authority enters a program. `World` is zero-sized, so
        // this parameter costs nothing at the machine level -- the C entry
        // point still calls `main` with no arguments.
        let world = program.world();
        if entry.n_params != 1 || entry.slots.first() != Some(&world) {
            return Err(refused(format!(
                "{where_}: error: `main` takes one argument, the `World` the runtime hands it"
            )));
        }
        if entry.ret != lex_sys_types::Type::Int {
            return Err(refused(format!(
                "{where_}: error: `main` returns `int`, the process exit status"
            )));
        }
    } else {
        return Err(refused(format!("{where_}: error: no `main` function")));
    }

    Ok(program)
}

/// Print every unit's content hash.
///
/// The program is checked first: hashing something that does not compile would
/// hand out an identity for a thing that is not a program.
fn print_ids(inputs: &[PathBuf], with_std: bool) -> Result<(), Failure> {
    let (ast, map) = parse_program(inputs, with_std)?;
    lex_sys_ir::lower(&ast).map_err(|d| refused(d.render_in(&map)))?;

    let identities = lex_sys_id::identify(&ast);

    // Written through a locked handle rather than `println!`, which panics on
    // a closed pipe: `lex-sys ids big.ls | head` is an ordinary thing to do,
    // and a backtrace is the wrong answer to it.
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let written = (|| -> io::Result<()> {
        for decl in &identities.types {
            writeln!(out, "type {:<24} {}", decl.name, decl.id)?;
        }
        for func in &identities.functions {
            writeln!(out, "sig  {:<24} {}", func.name, func.sig)?;
            writeln!(out, "body {:<24} {}", func.name, func.body)?;
        }
        out.flush()
    })();

    match written {
        Ok(()) => Ok(()),
        // The reader stopped listening. That is their business, not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(environment(format!("cannot write to stdout: {e}"))),
    }
}

fn build(inputs: &[PathBuf], output: &Path, emit: Emit, with_std: bool) -> Result<(), Failure> {
    let program = compile_to_ir(inputs, with_std)?;
    let object = lex_sys_codegen::compile_object(&program, "main")
        .map_err(|e| environment(format!("code generation failed: {e}")))?;

    match emit {
        Emit::Obj => std::fs::write(output, &object)
            .map_err(|e| environment(format!("cannot write `{}`: {e}", output.display()))),
        Emit::Exe => {
            let object_path = output.with_extension("o");
            std::fs::write(&object_path, &object).map_err(|e| {
                environment(format!("cannot write `{}`: {e}", object_path.display()))
            })?;
            let result = link(&object_path, output);
            let _ = std::fs::remove_file(&object_path);
            result
        }
    }
}

/// Link with the platform C toolchain.
///
/// M0 shells out to `cc` rather than driving a linker itself: the C runtime
/// provides `_start` and `putchar`, and "no C" is a much later goal than "no
/// Rust" (#1).
fn link(object: &Path, output: &Path) -> Result<(), Failure> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let status = Command::new(&cc)
        .arg(object)
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|e| environment(format!("cannot run the linker `{cc}`: {e}")))?;
    if !status.success() {
        return Err(environment(format!("the linker `{cc}` failed with {status}")));
    }
    Ok(())
}
