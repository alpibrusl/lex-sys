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

use lex_sys_ir::TypeInfo;
use lex_sys_syntax::{Ast, SourceFile, SourceMap};
use lex_sys_types::{DefId, Type};

const USAGE: &str = "\
lex-sys — the bootstrap compiler for the lex-sys systems dialect

usage:
    lex-sys build <file.ls>... [-o <output>] [--emit exe|obj] [--std]
    lex-sys check <file.ls>... [--std]
    lex-sys run   <file.ls>... [--std]
    lex-sys ids   <file.ls>... [--std]
    lex-sys authority <file.ls>... [--std] [--output json]
    lex-sys layout    <file.ls>... [--std]
    lex-sys print <file.ls>
    lex-sys --version

options:
    -o <output>     where to write the result (default: the first input's stem)
    --emit exe|obj  emit a linked executable (default) or a bare object file
    --std           make the standard library's source available
    --output json   `authority` as data rather than prose

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
            let Invocation { inputs, with_std, .. } = parse_args(&args[1..], false)?;
            compile_to_ir(&inputs, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        // `docs/many-files.md` §5: printing is about text, and text is
        // what a file is -- so this renders exactly one.
        "print" => {
            let Invocation { inputs, .. } = parse_args(&args[1..], false)?;
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
            let Invocation { inputs, with_std, .. } = parse_args(&args[1..], false)?;
            print_ids(&inputs, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        "authority" => {
            let Invocation { inputs, with_std, json, .. } = parse_args(&args[1..], false)?;
            print_authority(&inputs, with_std, json)?;
            Ok(ExitCode::SUCCESS)
        }
        "layout" => {
            let Invocation { inputs, with_std, .. } = parse_args(&args[1..], false)?;
            print_layout(&inputs, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        "build" => {
            let Invocation { inputs, output, emit, with_std, .. } = parse_args(&args[1..], true)?;
            let output = output.unwrap_or_else(|| default_output(&inputs[0], emit));
            build(&inputs, &output, emit, with_std)?;
            Ok(ExitCode::SUCCESS)
        }
        "run" => {
            let Invocation { inputs, with_std, .. } = parse_args(&args[1..], false)?;
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
    ("<std>/bignum.ls", include_str!("../../../std/bignum.ls")),
    ("<std>/fmt.ls", include_str!("../../../std/fmt.ls")),
    ("<std>/utf8.ls", include_str!("../../../std/utf8.ls")),
];

/// What a command line asked for.
///
/// A struct rather than a tuple because the fifth field is where a tuple
/// stops being readable at the call site -- the same reason
/// `resolve_type_at` became a `Resolving`.
struct Invocation {
    inputs: Vec<PathBuf>,
    output: Option<PathBuf>,
    emit: Emit,
    with_std: bool,
    /// `--output json` (`docs/authority.md` §3): the report as data rather
    /// than as prose, for a consumer that checks it against a grant.
    json: bool,
}

fn parse_args(args: &[String], allow_output: bool) -> Result<Invocation, Failure> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut output = None;
    let mut emit = Emit::Exe;
    let mut with_std = false;
    let mut json = false;
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
            "--output" => match it.next().map(String::as_str) {
                Some("json") => json = true,
                Some(other) => return Err(usage(format!("unknown output form `{other}`"))),
                None => return Err(usage("`--output` needs a form")),
            },
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
    Ok(Invocation { inputs, output, emit, with_std, json })
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
/// A JSON string array, with each element escaped.
fn quoted(values: &[&str]) -> String {
    let each: Vec<String> = values.iter().map(|v| format!("\"{}\"", escaped(v))).collect();
    each.join(", ")
}

/// The two characters a JSON string may not carry raw. An effect label is
/// an identifier and a narrowing is a path or a library name, so neither
/// can contain a control character -- but escaping here rather than
/// trusting that is what keeps the output parseable if either widens.
fn escaped(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `lex-sys authority <file>` — what a program can do
/// (`docs/authority.md`).
///
/// The surface is the union of the declared rows of everything `main`
/// reaches, and that needs no new analysis: pass 2 already emits exactly
/// what `main` reaches (`standard-library.md` §5.2), so `Program::funcs`
/// *is* the reachable set and every `Func` carries its row. The same
/// reachability that decides what goes in the binary decides what the
/// binary can do, which is the reason these are one number rather than
/// two.
///
/// Rows are exact in both directions (§7.3), so this is precise rather
/// than conservative: a label here is an effect the program performs on
/// some path, not one it might.
/// `lex-sys layout` — what each declared type costs in memory
/// (`docs/layout.md` §4).
///
/// A report rather than a flag, because there is nothing to turn on: §2
/// of that document measured the layout and deliberately did not change
/// it. What the report is *for* is the trigger — when `size` and
/// `packed` differ on a program someone cares about, the deferral stops
/// being right, and this is how that gets noticed rather than
/// remembered.
fn print_layout(inputs: &[PathBuf], with_std: bool) -> Result<(), Failure> {
    let program = compile_to_ir(inputs, with_std)?;
    let triple = lex_sys_codegen::host_triple();

    // The prelude's eight -- `World`, the five capabilities, `Box` and
    // `Split` -- are not types the program declared, and a report about
    // a program's memory should not open with eight rows of zero.
    const PRELUDE: usize = lex_sys_ir::PRELUDE_SPLIT + 1;

    let mut rows: Vec<(String, lex_sys_codegen::Layout)> = Vec::new();
    for (index, info) in program.types.iter().enumerate().skip(PRELUDE) {
        let (name, members): (&String, Vec<&Type>) = match info {
            TypeInfo::Struct { name, fields } => (name, fields.iter().map(|(_, t)| t).collect()),
            TypeInfo::Enum { name, variants } => {
                (name, variants.iter().flat_map(|(_, p)| p.iter()).collect())
            }
        };
        // A generic declaration has no one layout -- `Pair[int, bool]`
        // and `Pair[bool, int]` are two -- so it is skipped rather than
        // measured at a substitution nobody wrote. Reporting each
        // instantiation instead would mean reporting names the source
        // does not contain, which is a worse answer than saying nothing.
        if members.iter().any(|t| mentions_parameter(t)) {
            continue;
        }
        let ty = Type::Named(DefId(index as u32), Vec::new());
        rows.push((name.clone(), lex_sys_codegen::layout_of(&ty, &program, &triple)));
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let written = (|| -> io::Result<()> {
        writeln!(
            out,
            "{:<24} {:>6} {:>7} {:>8} {:>8}",
            "type", "leaves", "size", "packed", "stride"
        )?;
        for (name, layout) in &rows {
            writeln!(
                out,
                "{:<24} {:>6} {:>7} {:>8} {:>8}",
                name, layout.leaves, layout.size, layout.packed, layout.stride
            )?;
        }
        // The one line that is the point of the report.
        let savings: u32 = rows.iter().map(|(_, l)| l.size.saturating_sub(l.packed)).sum();
        if savings > 0 {
            writeln!(
                out,
                "\npacking would save {savings} bytes per copy across these types (`docs/layout.md` §2)"
            )?;
        }
        out.flush()
    })();

    match written {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(environment(format!("cannot write to stdout: {e}"))),
    }
}

/// Does this type mention a generic parameter, at any depth?
fn mentions_parameter(ty: &Type) -> bool {
    match ty {
        Type::Param(_) => true,
        Type::Named(_, args) => args.iter().any(mentions_parameter),
        Type::Slice(inner) => mentions_parameter(inner),
        Type::Ref { inner, .. } => mentions_parameter(inner),
        Type::Tuple(parts) => parts.iter().any(mentions_parameter),
        _ => false,
    }
}

fn print_authority(inputs: &[PathBuf], with_std: bool, json: bool) -> Result<(), Failure> {
    let program = compile_to_ir(inputs, with_std)?;

    // Every distinct label the reachable set performs, with the value it
    // was narrowed to. Sorted so the report is a function of the program
    // rather than of declaration order.
    let mut performed: Vec<(String, Option<String>)> = Vec::new();
    for func in &program.funcs {
        for label in func.performs.labels() {
            let entry = (label.name.clone(), label.argument.clone());
            if !performed.contains(&entry) {
                performed.push(entry);
            }
        }
    }
    performed.sort();

    let labels: Vec<String> = performed
        .iter()
        .map(|(name, argument)| match argument {
            Some(value) => format!("{name}(\"{value}\")"),
            None => name.clone(),
        })
        .collect();

    // The distinct *kinds*, which is the coarse question a grant is
    // written against -- `lex-os-check`'s `CheckReport` draws the same
    // line, keeping the arguments separately for the precise one.
    let mut kinds: Vec<&str> = performed.iter().map(|(name, _)| name.as_str()).collect();
    kinds.dedup();

    let mut symbols: Vec<&str> =
        program.externs.iter().map(|declared| declared.symbol.as_str()).collect();
    symbols.sort_unstable();
    symbols.dedup();

    // The functions the checker can prove are pure (`docs/purity.md` §2).
    // Reported because it is a fact about the program that no other
    // language here can state: C's `__attribute__((const))` is an
    // unchecked promise and Rust has no way to say it at all. What it is
    // *for* is a backend that can use it, which Cranelift cannot (§4).
    let mut pure: Vec<&str> =
        program.funcs.iter().filter(|f| f.is_pure()).map(|f| f.name.as_str()).collect();
    pure.sort_unstable();
    let total = program.funcs.len();

    // What compile-time evaluation removed (`docs/compile-time.md` §9).
    // A pass that rewrites a program's own arithmetic should be able to
    // say how much of it it rewrote; reporting it is also the only
    // portable way to *test* that it happened, since reading the
    // instructions back needs a disassembler and CI has two platforms.
    let folded_operators: usize =
        program.funcs.iter().map(|f| f.folded).sum::<usize>() + program.folded_late;
    let folded_calls = program.folded_calls;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json {
        let written = (|| -> io::Result<()> {
            writeln!(out, "{{")?;
            writeln!(out, "  \"effects\": [{}],", quoted(&kinds))?;
            writeln!(out, "  \"labels\": [")?;
            for (i, (name, argument)) in performed.iter().enumerate() {
                let comma = if i + 1 == performed.len() { "" } else { "," };
                let argument = match argument {
                    Some(value) => format!("\"{}\"", escaped(value)),
                    None => "null".to_owned(),
                };
                writeln!(
                    out,
                    "    {{ \"name\": \"{}\", \"argument\": {argument} }}{comma}",
                    escaped(name)
                )?;
            }
            writeln!(out, "  ],")?;
            writeln!(out, "  \"foreign_symbols\": [{}],", quoted(&symbols))?;
            writeln!(out, "  \"pure\": [{}],", quoted(&pure))?;
            writeln!(out, "  \"folded_operators\": {folded_operators},")?;
            writeln!(out, "  \"folded_calls\": {folded_calls},")?;
            writeln!(out, "  \"functions\": {total}")?;
            writeln!(out, "}}")?;
            out.flush()
        })();
        return match written {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(environment(format!("cannot write to stdout: {e}"))),
        };
    }
    let written = (|| -> io::Result<()> {
        if labels.is_empty() {
            writeln!(out, "performs nothing")?;
        } else {
            writeln!(out, "performs")?;
            for label in &labels {
                writeln!(out, "    {label}")?;
            }
        }
        // The negative half, which is the one a capability language is for:
        // a reader wants to know what a program *cannot* do, and an absent
        // label is exactly that.
        let untouched: Vec<&str> = [
            ("the console", ["io_read", "io_write"].as_slice()),
            ("the filesystem", ["fs_read", "fs_write"].as_slice()),
            ("the heap", ["heap"].as_slice()),
            ("the command line", ["args"].as_slice()),
            ("foreign code", ["ffi"].as_slice()),
        ]
        .into_iter()
        .filter(|(_, names)| {
            !names.iter().any(|name| {
                labels.iter().any(|label| label == name || label.starts_with(&format!("{name}(")))
            })
        })
        .map(|(what, _)| what)
        .collect();
        if !untouched.is_empty() {
            writeln!(out, "never touches")?;
            for what in untouched {
                writeln!(out, "    {what}")?;
            }
        }
        if !symbols.is_empty() {
            writeln!(out, "foreign symbols")?;
            for symbol in symbols {
                writeln!(out, "    {symbol}")?;
            }
        }
        if !pure.is_empty() {
            writeln!(out, "provably pure ({} of {total})", pure.len())?;
            for name in &pure {
                writeln!(out, "    {name}")?;
            }
        }
        if folded_operators > 0 || folded_calls > 0 {
            writeln!(out, "evaluated at compile time")?;
            if folded_operators > 0 {
                writeln!(out, "    {folded_operators} operators")?;
            }
            if folded_calls > 0 {
                writeln!(out, "    {folded_calls} calls")?;
            }
        }
        out.flush()
    })();

    match written {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(environment(format!("cannot write to stdout: {e}"))),
    }
}

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
