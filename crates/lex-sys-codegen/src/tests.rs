use super::*;
use lex_sys_ir::lower;
use lex_sys_syntax::parse;
use object::{Object, ObjectSymbol, SymbolKind};

/// `docs/internal-errors.md` §5: a checked program, with one
/// function's IR broken in a way the front end never produces.
fn broken(break_it: impl FnOnce(&mut lex_sys_ir::Func)) -> (Program, usize) {
    let source = "fn seven() -> [] int { return 7; } \
                      fn main(world: World) -> [] int { release(world); return seven() - 7; }";
    // Only `seven`'s *body* is broken, never its signature, so its
    // caller is untouched: the failure is its own, and the error has
    // to say so.
    let mut program = lower(&parse(source).expect("should parse")).expect("should lower");
    let index =
        program.funcs.iter().position(|f| f.name == "seven").expect("`seven` reaches the IR");
    break_it(&mut program.funcs[index]);
    (program, index)
}

/// The verifier's refusal names the function it refused.
///
/// #71's bug in its smallest form: a value of the wrong machine width
/// where the signature wants another -- here a `bool`, one byte,
/// returned from a function declared to return a 64-bit `int`.
#[test]
fn a_backend_failure_names_its_function() {
    let (program, seven) = broken(|f| {
        f.body = vec![lex_sys_ir::Stmt::Return(lex_sys_ir::Expr::Bool(true))];
    });
    let error = compile_object(&program, "main").expect_err("the verifier should refuse it");
    assert_eq!(error.function, Some(seven), "{error}");
    assert!(error.message.contains("Verifier"), "{error}");
}

/// A panic in the backend is caught at the function and becomes the
/// same error, carrying the panic's message rather than unwinding out
/// of the compiler.
///
/// `len` is lowered as its own node, so a call to the `len` builtin
/// reaching the backend is exactly one of the "the checker should have
/// refused it" invariants.
#[test]
fn a_backend_panic_names_its_function() {
    let (program, seven) = broken(|f| {
        f.body = vec![lex_sys_ir::Stmt::Return(lex_sys_ir::Expr::Call {
            callee: lex_sys_ir::Callee::Builtin(lex_sys_ir::Builtin::Len),
            args: vec![lex_sys_ir::Expr::Int(0)],
        })];
    });
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = compile_object(&program, "main");
    std::panic::set_hook(hook);
    let error = result.expect_err("the backend should refuse it");
    assert_eq!(error.function, Some(seven), "{error}");
    assert!(error.message.contains("`len` is lowered as `Expr::Len`"), "{error}");
}

const SOURCE: &str = "fn shout[&i](io: &!i Io) -> [io_write] int { return putchar(io, 33); } \
                          fn main(world: World) -> [] int { \
                              let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(ffi); \
                              var status = 0; \
                              borrow mut io as &!i in { status = shout(i); } \
                              release(io); \
                              return status; \
                          }";

/// The two targets to check: this host's architecture, once per binary
/// format, paired with the symbol prefix that format calls for.
///
/// Cranelift compiles in only the host's backend, so the architecture has
/// to be this host's — but the *format* need not be, which is what makes
/// Mach-O's conventions testable from Linux and ELF's from darwin.
fn targets() -> [(String, &'static str); 2] {
    let arch = host_triple().architecture.to_string();
    [(format!("{arch}-unknown-linux-gnu"), ""), (format!("{arch}-apple-darwin"), "_")]
}

/// Compile for a target and read back the object's symbol table.
/// Each symbol's name, whether it is global, and whether this object
/// *defines* it. An import is global and undefined; an export is global
/// and defined, and the difference is what "only `main` is exported"
/// means once the backend reaches for libc on its own.
fn symbols(triple: &str) -> Vec<(String, bool, bool)> {
    symbols_of(SOURCE, triple)
}

fn symbols_of(source: &str, triple: &str) -> Vec<(String, bool, bool)> {
    let ast = parse(source).expect("should parse");
    let program = lower(&ast).expect("should lower");
    let bytes = compile_object_for(&program, "main", triple.parse().expect("a valid triple"))
        .expect("should compile");
    let file = object::File::parse(&*bytes).expect("a readable object file");
    // Text symbols are the functions; an undefined import reads back as
    // `Unknown`, and section and file symbols are neither.
    let mut names: Vec<(String, bool, bool)> = file
        .symbols()
        .filter(|s| matches!(s.kind(), SymbolKind::Text | SymbolKind::Unknown))
        .filter_map(|s| s.name().ok().map(|n| (n.to_owned(), s.is_global(), !s.is_undefined())))
        .filter(|(name, _, _)| !name.is_empty())
        .collect();
    names.sort();
    names
}

fn names(triple: &str) -> Vec<String> {
    symbols(triple).into_iter().map(|(n, _, _)| n).collect()
}

/// A symbol is spelled as it was declared, plus whatever the platform adds
/// — and nothing more.
///
/// `object` picks a `Mangling` from the binary format when the object is
/// created and applies the Mach-O leading underscore itself, without being
/// asked. Prefixing here as well produced `__main`, and the darwin linker,
/// looking for `_main`, could not resolve it. Linux never sees this, which
/// is exactly why the assertion covers both formats from any host.
#[test]
fn symbols_are_spelled_for_their_platform_and_prefixed_once() {
    for (triple, prefix) in targets() {
        let names = names(&triple);
        for base in ["main", "lexs_main", "lexs_shout", "putchar"] {
            let expected = format!("{prefix}{base}");
            assert!(
                names.contains(&expected),
                "{triple} should define `{expected}`, got {names:?}"
            );
        }
        // libc spells macOS's `stdout` as `__stdoutp` and its
        // `stderr` as `__stderrp`, so the Mach-O symbols are
        // `___stdoutp` and `___stderrp` — three underscores, prefixed once,
        // and correct (`docs/bulk-io.md` §3). The check below is a
        // proxy for "prefixed once" and cannot tell that apart from
        // the bug, so the one name that is legitimately spelled with
        // underscores is named here rather than the guard weakened.
        let spelled_with_underscores = ["___stdoutp", "__stdoutp", "___stderrp", "__stderrp"];
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("__") && !spelled_with_underscores.contains(&n.as_str())),
            "{triple}: a doubly-prefixed symbol is an unresolvable link: {names:?}"
        );
    }
}

/// `main` is the only symbol the linker may bind from outside. Everything
/// the program defines is local and carries the `lexs_` prefix, so a
/// lex-sys function called `write` or `exit` cannot collide with libc's.
/// A borrowing program reaches real instruction selection on every target
/// we ship, not just the host's.
///
/// `borrow` is the first thing in the language that needs an address:
/// `stack_addr` plus loads through a pointer, where a pointer's width is
/// the target's rather than a constant. Emitting for both formats is the
/// cheapest way to find out that the layout code disagrees with one.
///
/// Only the host's architecture is reachable here, because Cranelift
/// builds one backend by default; aarch64 emission for this program was
/// checked by hand with `cranelift-codegen`'s `arm64` feature turned on,
/// and CI runs the whole suite natively on darwin-aarch64 anyway.
#[test]
fn a_borrow_lowers_on_every_target() {
    const BORROWING: &str = "\
            struct Wide { a: int, b: bool, c: int } \
            fn look[&r](w: &r Wide) -> [] int { return w.a + w.c; } \
            fn main() -> [] int { let w = Wide { a: 1, b: true, c: 2 }; \
            borrow w as &r in { return look(r) - 3; } }";
    for (triple, _) in targets() {
        let ast = parse(BORROWING).expect("should parse");
        let program = lower(&ast).expect("should lower");
        let triple: Triple = triple.parse().expect("a valid triple");
        compile_object_for(&program, "main", triple.clone())
            .unwrap_or_else(|e| panic!("`{triple}` should emit: {e}"));
    }
}

/// §8.4: a foreign declaration becomes an import under the symbol it
/// named, and the capability that authorised it does not travel.
///
/// That the capability does not travel is what `tests/accept/
/// narrowed_capability.ls` proves end to end: it calls `labs(-7)` and
/// prints `7`, which it could not do if a zero-sized capability were
/// pushed in front of the integer. That failure mode is not
/// hypothetical — `putchar` printed `0xA0` three times before
/// `erased_args` existed.
#[test]
fn a_foreign_declaration_becomes_an_import_and_its_capability_does_not() {
    const FOREIGN: &str = "\
            extern fn labs[&f](ffi: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int; \
            fn main(world: World) -> [] int { \
                let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(fs); release(io); \
                let libc = narrow(ffi, \"libc\"); var n = 0; \
                borrow libc as &f in { n = labs(f, 0 - 7); } \
                release(libc); return n - 7; \
            }";
    for (triple, prefix) in targets() {
        let names: Vec<String> =
            symbols_of(FOREIGN, &triple).into_iter().map(|(n, _, _)| n).collect();
        let expected = format!("{prefix}labs");
        assert!(names.contains(&expected), "{triple} should import `{expected}`: {names:?}");
        assert!(
            !names.iter().any(|n| n.contains("narrow") || n.contains("Ffi")),
            "{triple}: narrowing is a compile-time fact and emits nothing: {names:?}"
        );
    }
}

/// §6: an arena is one `malloc` in and one `free` out, on every target.
///
/// The symbol check is the cheap half. The real assertion is that the
/// body emits at all: `region` and `alloc` are the first constructs that
/// build a pointer from a call result and bump it, and pointer width is
/// the target's rather than a constant — the same class of mistake that
/// `a_borrow_lowers_on_every_target` exists to catch.
#[test]
fn an_arena_reaches_libc_on_every_target() {
    const ARENA: &str = "\
            struct Node { value: int, tag: bool } \
            fn value_of[&r](n: &r Node) -> [] int { return n.value; } \
            fn main() -> [] int { \
                var total = 0; \
                region a { \
                    let first = alloc[a](Node { value: 1, tag: true }); \
                    first.value = first.value + 1; \
                    region inner { \
                        let second = alloc[inner](Node { value: 2, tag: false }); \
                        total = value_of(first) + value_of(second); \
                    } \
                } \
                return total - 4; \
            }";
    for (triple, prefix) in targets() {
        let with_arena: Vec<String> =
            symbols_of(ARENA, &triple).into_iter().map(|(n, _, _)| n).collect();
        for base in ["malloc", "free"] {
            let expected = format!("{prefix}{base}");
            assert!(
                with_arena.contains(&expected),
                "{triple} should import `{expected}`: {with_arena:?}"
            );
        }

        // And a program with no arena imports neither, which is what
        // makes the assertion above mean something.
        let plain = names(&triple);
        assert!(
            !plain.iter().any(|n| n.contains("malloc") || n.contains("free")),
            "{triple}: a program with no `region` should not reach the allocator: {plain:?}"
        );
    }
}

/// A slice is two leaves, and the loop that fills one lowers on every
/// target.
///
/// `alloc_slice` is the first construct that emits a *loop the backend
/// wrote* rather than one the program did, with a runtime trip count
/// and a stride that depends on the element's layout. Pointer width is
/// the target's, so emitting for both formats is the cheap way to find
/// out that the address arithmetic disagrees with one.
#[test]
fn a_slice_lowers_on_every_target() {
    const SLICES: &str = "\
            struct Cell { value: int, tag: bool } \
            fn total[&r](xs: &r [Cell]) -> [] int { \
                var sum = 0; var i = 0; \
                while i < len(xs) { sum = sum + xs[i].value; i = i + 1; } \
                return sum; \
            } \
            fn main() -> [] int { \
                var answer = 0; \
                region a { \
                    let xs = alloc_slice[a](4, Cell { value: 0, tag: false }); \
                    var i = 0; \
                    while i < len(xs) { xs[i] = Cell { value: i, tag: true }; i = i + 1; } \
                    answer = total(xs); \
                } \
                return answer - 6; \
            }";
    for (triple, _) in targets() {
        let ast = parse(SLICES).expect("should parse");
        let program = lower(&ast).expect("should lower");
        let triple: Triple = triple.parse().expect("a valid triple");
        compile_object_for(&program, "main", triple.clone())
            .unwrap_or_else(|e| panic!("`{triple}` should emit: {e}"));
    }
}

#[test]
fn only_the_entry_point_is_global() {
    for (triple, _) in targets() {
        let triple = triple.as_str();
        for (name, global, _) in symbols(triple) {
            if name.contains("lexs_") {
                assert!(!global, "{triple}: `{name}` should be local");
            }
        }
        // libc's symbols are global too, but this object imports them
        // rather than defining them, so they are not exports.
        let globals: Vec<String> = symbols(triple)
            .into_iter()
            .filter(|(_, global, defined)| *global && *defined)
            .map(|(name, _, _)| name)
            .collect();
        assert_eq!(globals.len(), 1, "{triple}: exactly one exported symbol, got {globals:?}");
        assert!(globals[0].ends_with("main"), "{triple}: {globals:?}");
    }
}
