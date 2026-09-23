//! A mutation fuzzer for the front end and the backend
//! (`docs/fuzzing.md`).
//!
//! Every `.ls` file in the repository is a seed. A mutant is a seed with a
//! few token-level edits: a token deleted, duplicated, swapped with
//! another, replaced by one from anywhere in the corpus, or a run of
//! tokens spliced in from another file. Each mutant goes through the
//! whole pipeline, and three properties must hold:
//!
//! 1. **Nothing panics.** Parsing, checking and code generation each
//!    answer `Ok` or a refusal; a panic anywhere is a bug.
//! 2. **What parses, prints and parses again** to the same tree, and so
//!    to the same canonical text.
//! 3. **What the checker accepts, the backend compiles.** A backend
//!    failure on an accepted program is an `internal` refusal
//!    (`docs/internal-errors.md`), and each one is a bug.
//!
//! Stable Rust, no dependencies, and deterministic: a seed and an
//! iteration count reproduce a run exactly. `LEX_SYS_FUZZ_ITERATIONS` and
//! `LEX_SYS_FUZZ_SEED` override the defaults for a long run.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use lex_sys_syntax::Ast;

/// The standard library, as `lex-sys --std` supplies it.
const STD: &[&str] = &[
    include_str!("../../../std/bytes.ls"),
    include_str!("../../../std/math.ls"),
    include_str!("../../../std/io.ls"),
    include_str!("../../../std/buffer.ls"),
    include_str!("../../../std/option.ls"),
    include_str!("../../../std/result.ls"),
    include_str!("../../../std/list.ls"),
    include_str!("../../../std/vec.ls"),
    include_str!("../../../std/bignum.ls"),
    include_str!("../../../std/fmt.ls"),
    include_str!("../../../std/utf8.ls"),
    include_str!("../../../std/flags.ls"),
];

/// xorshift64*: small, fast, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the workspace root is two levels above this crate")
}

fn seeds() -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![repo_root()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a readable directory") {
            let path = entry.expect("a readable entry").path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "ls") {
                found.push(path);
            }
        }
    }
    found.sort();
    found.iter().map(|p| std::fs::read_to_string(p).expect("a readable seed")).collect()
}

/// A seed as its tokens: each one's kind and text. A seed that does not
/// lex (the lexer's own reject fixtures) contributes nothing.
fn tokens(source: &str) -> Vec<(String, String)> {
    match lex_sys_syntax::lexer::tokenize(source) {
        Ok(tokens) => tokens
            .iter()
            .filter(|t| t.span.end > t.span.start)
            .map(|t| {
                (
                    format!("{:?}", t.kind),
                    source[t.span.start as usize..t.span.end as usize].to_owned(),
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Every token in the corpus, grouped by kind, so a replacement can keep
/// the shape of what it replaces.
type Pool = std::collections::BTreeMap<String, Vec<String>>;

/// A few edits to a seed. Half of them replace a token with another of
/// the **same kind** -- a name for a name, a number for a number, an
/// operator for an operator -- which keeps the program parsing far more
/// often than a random edit does, and so reaches the checker and the
/// backend. The rest are structural: delete, duplicate, swap, or splice a
/// run of tokens from another file.
fn mutate(
    rng: &mut Rng,
    seed: &[(String, String)],
    corpus: &[Vec<(String, String)>],
    pool: &Pool,
) -> Vec<String> {
    let mut out = seed.to_vec();
    let edits = if rng.below(4) == 0 { 2 + rng.below(2) } else { 1 };
    for _ in 0..edits {
        if out.is_empty() {
            break;
        }
        let at = rng.below(out.len());
        match rng.below(10) {
            0..=4 => {
                let same = &pool[&out[at].0];
                out[at].1 = same[rng.below(same.len())].clone();
            }
            5 => {
                out.remove(at);
            }
            6 => {
                let t = out[at].clone();
                out.insert(at, t);
            }
            7 => {
                let other = rng.below(out.len());
                out.swap(at, other);
            }
            _ => {
                let donor = &corpus[rng.below(corpus.len())];
                let from = rng.below(donor.len());
                let len = 1 + rng.below(8.min(donor.len() - from));
                out.splice(at..at, donor[from..from + len].iter().cloned());
            }
        }
    }
    out.into_iter().map(|(_, text)| text).collect()
}

/// What one mutant did, when it did something it must not.
struct Finding {
    stage: &'static str,
    message: String,
    source: String,
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

fn parse_with_std(source: &str) -> Result<Ast, lex_sys_syntax::Diagnostic> {
    let mut ast = Ast::new();
    lex_sys_syntax::parse_into(&mut ast, source, 0)?;
    let mut base = source.len() as u32 + 1;
    for text in STD {
        lex_sys_syntax::parse_into(&mut ast, text, base).expect("the standard library parses");
        base += text.len() as u32 + 1;
    }
    Ok(ast)
}

/// How far mutants got, so a run that never reaches the checker or the
/// backend cannot pass for one that tested them.
#[derive(Default, Debug)]
struct Reach {
    parsed: u64,
    checked: u64,
    compiled: u64,
}

/// Run one program through the pipeline, and say what went wrong if
/// anything did that a refusal does not explain.
fn exercise(source: &str, reach: &mut Reach) -> Option<Finding> {
    let finding =
        |stage, message: String| Some(Finding { stage, message, source: source.to_owned() });
    let parsed = match catch_unwind(|| lex_sys_syntax::parse(source)) {
        Err(p) => return finding("parse", panic_text(p.as_ref())),
        Ok(parsed) => parsed,
    };
    let Ok(alone) = parsed else { return None };
    reach.parsed += 1;

    // Property 2: the canonical text parses again, to the same tree and so
    // to the same text. The tree is the stronger half: a printer that loses
    // a parenthesis can still reach a fixed point, one reprint later, on a
    // program that means something else.
    let printed = match catch_unwind(AssertUnwindSafe(|| lex_sys_syntax::print(&alone))) {
        Err(p) => return finding("print", panic_text(p.as_ref())),
        Ok(text) => text,
    };
    match lex_sys_syntax::parse(&printed) {
        Err(d) => return finding("reparse", format!("printed text does not parse: {}", d.message)),
        Ok(again) if !same_tree(&alone, &again) => {
            let message = "the printed text is a different tree".to_owned();
            return Some(Finding {
                stage: "reparse",
                message,
                source: format!("{}\n{source}", first_difference(&alone, &again)),
            });
        }
        Ok(again) if lex_sys_syntax::print(&again) != printed => {
            return finding("reparse", "printing is not a fixed point".to_owned());
        }
        Ok(_) => {}
    }

    let Ok(ast) = parse_with_std(source) else { return None };
    let program = match catch_unwind(AssertUnwindSafe(|| lex_sys_ir::lower_all(&ast))) {
        Err(p) => return finding("check", panic_text(p.as_ref())),
        Ok(Err(_)) => return None,
        Ok(Ok(program)) => program,
    };
    reach.checked += 1;

    // The CLI refuses a program without a `main` of the right shape before
    // the backend sees it, so the fuzzer does too.
    let entry = program.find("main")?;
    let entry = program.func(entry);
    if entry.n_params != 1
        || entry.slots.first() != Some(&program.world())
        || entry.ret != lex_sys_types::Type::Int
    {
        return None;
    }
    match catch_unwind(AssertUnwindSafe(|| lex_sys_codegen::compile_object(&program, "main"))) {
        Err(p) => finding("backend", panic_text(p.as_ref())),
        Ok(Err(e)) => finding("backend", format!("an accepted program did not compile: {e}")),
        Ok(Ok(_)) => {
            reach.compiled += 1;
            None
        }
    }
}

/// The tree as text with every symbol spelled out. Symbols are numbered in
/// the order the parser first meets a name, and the printer writes some
/// lists in a canonical order of its own -- `[&h, T]` as `[T, &h]` -- so
/// the same tree can come back numbered differently.
fn spelled(ast: &Ast) -> String {
    let raw = format!("{:?}{:?}{:?}{:?}", ast.exprs, ast.stmts, ast.types, ast.items);
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw.as_str();
    while let Some(at) = rest.find("Symbol(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "Symbol(".len()..];
        let end = after.find(')').expect("a closed Symbol");
        let index: u32 = after[..end].parse().expect("a symbol index");
        out.push_str(&format!("{:?}", ast.symbols.resolve(lex_sys_syntax::ast::Symbol(index))));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The same tree: equal tables, or equal once symbols are spelled out.
fn same_tree(a: &Ast, b: &Ast) -> bool {
    (a.exprs == b.exprs && a.stmts == b.stmts && a.types == b.types && a.items == b.items)
        || spelled(a) == spelled(b)
}

/// Where two trees first differ, as `//` lines that head the finding's
/// file. The message stays fixed so findings still deduplicate; this is
/// what makes one quick to minimize.
fn first_difference(before: &Ast, after: &Ast) -> String {
    fn first<T: PartialEq + std::fmt::Debug>(table: &str, a: &[T], b: &[T]) -> Option<String> {
        let at = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i))?;
        Some(format!(
            "// {table}[{at}]\n//   before: {:?}\n//   after:  {:?}",
            a.get(at),
            b.get(at)
        ))
    }
    first("exprs", &before.exprs, &after.exprs)
        .or_else(|| first("stmts", &before.stmts, &after.stmts))
        .or_else(|| first("types", &before.types, &after.types))
        .or_else(|| first("items", &before.items, &after.items))
        .unwrap_or_default()
}

/// Decimal or `0x` hex. A value that does not parse is refused rather than
/// replaced by the default: a run that silently ignored its seed would
/// report the default seed's result as if it were a new one.
fn env_number(name: &str, default: u64) -> u64 {
    let Ok(text) = std::env::var(name) else { return default };
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(&hex.replace('_', ""), 16),
        None => text.replace('_', "").parse(),
    };
    parsed.unwrap_or_else(|_| panic!("{name}={text:?} is not a number"))
}

/// `LEX_SYS_FUZZ_REPLAY=<file>` runs one saved finding through the same
/// pipeline, to check a fix or cut a finding down by hand.
#[test]
fn a_saved_finding_replays() {
    let Ok(path) = std::env::var("LEX_SYS_FUZZ_REPLAY") else { return };
    let source = std::fs::read_to_string(&path).expect("a readable finding");
    let found = exercise(&source, &mut Reach::default());
    if let Some(f) = found {
        panic!(
            "[{}] {}\n{}",
            f.stage,
            f.message,
            f.source.lines().take_while(|l| l.starts_with("//")).collect::<Vec<_>>().join("\n")
        );
    }
}

#[test]
fn mutants_never_crash_the_compiler() {
    let iterations = env_number("LEX_SYS_FUZZ_ITERATIONS", 3_000);
    let seed = env_number("LEX_SYS_FUZZ_SEED", 0x5eed_1e55);
    let corpus: Vec<Vec<(String, String)>> =
        seeds().iter().map(|s| tokens(s)).filter(|t| !t.is_empty()).collect();
    let mut pool = Pool::new();
    for (kind, text) in corpus.iter().flatten() {
        pool.entry(kind.clone()).or_default().push(text.clone());
    }
    assert!(!corpus.is_empty(), "no seeds");

    // A panic is caught and reported, so the default hook's message would
    // only be noise in the test output.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng(seed | 1);
    let mut findings: Vec<Finding> = Vec::new();
    let mut reach = Reach::default();
    for _ in 0..iterations {
        let seed_tokens = &corpus[rng.below(corpus.len())];
        let mutant = mutate(&mut rng, seed_tokens, &corpus, &pool).join(" ");
        if let Some(found) = exercise(&mutant, &mut reach) {
            // One report per distinct failure: the same panic from a
            // thousand mutants is one bug.
            if !findings.iter().any(|f| f.stage == found.stage && f.message == found.message) {
                findings.push(found);
            }
        }
    }
    std::panic::set_hook(hook);
    eprintln!("{iterations} mutants: {reach:?}");

    // Floors a quarter of what the mutator measured (`docs/fuzzing.md` §3):
    // 44% parse, 9% check, 8% compile. A change that made mutants stop
    // reaching the backend would otherwise pass by testing nothing.
    assert!(reach.parsed >= iterations / 10, "too few mutants parse: {reach:?}");
    assert!(reach.checked >= iterations / 50, "too few mutants check: {reach:?}");
    assert!(reach.compiled >= iterations / 50, "too few mutants compile: {reach:?}");

    if !findings.is_empty() {
        let dir = std::env::temp_dir().join("lex-sys-fuzz");
        let _ = std::fs::create_dir_all(&dir);
        let mut report = String::new();
        for (i, f) in findings.iter().enumerate() {
            let path = dir.join(format!("finding-{i}.ls"));
            let _ = std::fs::write(&path, &f.source);
            report.push_str(&format!("[{}] {} -- {}\n", f.stage, f.message, path.display()));
        }
        panic!(
            "{} distinct failures in {iterations} mutants (seed {seed:#x}):\n{report}",
            findings.len()
        );
    }
}
