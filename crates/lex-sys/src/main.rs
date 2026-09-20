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

use lex_sys_syntax::{SourceFile, parse};

const USAGE: &str = "\
lex-sys — the bootstrap compiler for the lex-sys systems dialect

usage:
    lex-sys build <file.ls> [-o <output>] [--emit exe|obj]
    lex-sys check <file.ls>
    lex-sys run   <file.ls>
    lex-sys ids   <file.ls>
    lex-sys print <file.ls>
    lex-sys --version

options:
    -o <output>     where to write the result (default: the input's stem)
    --emit exe|obj  emit a linked executable (default) or a bare object file

`ids` prints each declaration's content hash: a signature and a body for
every function, one identity for every type. See docs/canonical-ast.md.

`print` renders the parsed unit in canonical form. It is the AST-to-text
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
            let (input, _, _) = parse_args(&args[1..], false)?;
            compile_to_ir(&input)?;
            Ok(ExitCode::SUCCESS)
        }
        "print" => {
            let (input, _, _) = parse_args(&args[1..], false)?;
            let text = std::fs::read_to_string(&input)
                .map_err(|e| environment(format!("cannot read `{}`: {e}", input.display())))?;
            let file = SourceFile::new(input.display().to_string(), text);
            let ast = parse(&file.text).map_err(|d| refused(d.render(&file)))?;
            print!("{}", lex_sys_syntax::print(&ast));
            Ok(ExitCode::SUCCESS)
        }
        "ids" => {
            let (input, _, _) = parse_args(&args[1..], false)?;
            print_ids(&input)?;
            Ok(ExitCode::SUCCESS)
        }
        "build" => {
            let (input, output, emit) = parse_args(&args[1..], true)?;
            let output = output.unwrap_or_else(|| default_output(&input, emit));
            build(&input, &output, emit)?;
            Ok(ExitCode::SUCCESS)
        }
        "run" => {
            let (input, _, _) = parse_args(&args[1..], false)?;
            let dir = std::env::temp_dir().join(format!("lex-sys-run-{}", std::process::id()));
            std::fs::create_dir_all(&dir)
                .map_err(|e| environment(format!("cannot create `{}`: {e}", dir.display())))?;
            let exe = dir.join(default_output(&input, Emit::Exe).file_name().unwrap_or_default());
            let result = build(&input, &exe, Emit::Exe).and_then(|()| {
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

fn parse_args(
    args: &[String],
    allow_output: bool,
) -> Result<(PathBuf, Option<PathBuf>, Emit), Failure> {
    let mut input = None;
    let mut output = None;
    let mut emit = Emit::Exe;
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
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option `{other}`")));
            }
            other if input.is_none() => input = Some(PathBuf::from(other)),
            other => return Err(usage(format!("unexpected argument `{other}`"))),
        }
    }

    let input = input.ok_or_else(|| usage("no input file given"))?;
    Ok((input, output, emit))
}

fn default_output(input: &Path, emit: Emit) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default();
    match emit {
        Emit::Exe => PathBuf::from(stem),
        Emit::Obj => PathBuf::from(format!("{}.o", stem.to_string_lossy())),
    }
}

/// Read, parse and lower a file, reporting any refusal with its source line.
fn compile_to_ir(input: &Path) -> Result<lex_sys_ir::Program, Failure> {
    let text = std::fs::read_to_string(input)
        .map_err(|e| environment(format!("cannot read `{}`: {e}", input.display())))?;
    let file = SourceFile::new(input.display().to_string(), text);

    let ast = parse(&file.text).map_err(|d| refused(d.render(&file)))?;
    let program = lex_sys_ir::lower(&ast).map_err(|d| refused(d.render(&file)))?;

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
                "{}: error: `main` takes one argument, the `World` the runtime hands it",
                file.path
            )));
        }
        if entry.ret != lex_sys_types::Type::Int {
            return Err(refused(format!(
                "{}: error: `main` returns `int`, the process exit status",
                file.path
            )));
        }
    } else {
        return Err(refused(format!("{}: error: no `main` function", file.path)));
    }

    Ok(program)
}

/// Print every unit's content hash.
///
/// The program is checked first: hashing something that does not compile would
/// hand out an identity for a thing that is not a program.
fn print_ids(input: &Path) -> Result<(), Failure> {
    let text = std::fs::read_to_string(input)
        .map_err(|e| environment(format!("cannot read `{}`: {e}", input.display())))?;
    let file = SourceFile::new(input.display().to_string(), text);

    let ast = parse(&file.text).map_err(|d| refused(d.render(&file)))?;
    lex_sys_ir::lower(&ast).map_err(|d| refused(d.render(&file)))?;

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

fn build(input: &Path, output: &Path, emit: Emit) -> Result<(), Failure> {
    let program = compile_to_ir(input)?;
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
