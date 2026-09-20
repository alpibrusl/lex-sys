# The v0 compiler

Decisions M0 had to settle to exist, recorded here so they are not re-litigated
by accident. Each was an open question in [#1](https://github.com/alpibrusl/lex-sys/issues/1).

## Bootstrap host language: Rust

The v0 compiler is written in Rust. The alternative was hosting it in current
Lex.

Rust wins for one reason that outweighs the rest: **Cranelift is a Rust
library.** Hosting in Lex means reaching a code generator through FFI on day
one, which is the part of the pipeline M0 exists to prove. Rust also gives the
bootstrap compiler the same build and test story as `lex-os`, so nobody has to
learn a second one.

This is a bootstrap decision, not a permanent one. Self-hosting remains *a
spike before it is a plan* (#1): the cheap experiment is porting the `lex-ast` /
`lex-vcs` canonical forms and checking byte-identical `OpId`/`SigId`/`StageId`
over the existing op-log corpus. Nothing in this compiler's design assumes it
stays in Rust.

## File extension: `.ls`

`.ls` over `.lxs`. Shorter, and the collision risk (LiveScript) is not one this
ecosystem will meet in practice. `lex-sys build hello.ls`.

## Layout

```
crates/lex-sys-syntax    lexer, canonical-shaped AST, parser
crates/lex-sys-ir        resolution and well-formedness checks; the M0 IR
crates/lex-sys-codegen   Cranelift lowering, native object emission
crates/lex-sys           the CLI
examples/                programs that are meant to be read
tests/accept             fixtures that must compile and run
tests/reject             fixtures that must be refused, each stating why
```

Everything the compiler can refuse, it refuses in `lex-sys-ir`. The backend
takes IR that has already been proven well-formed, so Cranelift lowering has no
error path for a *program* — only for the environment.

## The M0 surface

> Historical: this is what M0 shipped. M1 adds `bool`, `&&`, `||` and `!`, and
> the rules below are unchanged by it.

One type (`int`, 64-bit signed), functions and calls, arithmetic, comparison,
`if`/`else`, `while`, and `let`/`var` bindings. That is the whole language.

Rules M0 already enforces, because they were cheaper to write than to retrofit:

- Every path through a function must `return`. The check is structural and
  conservative: a `while` never counts as a terminator, even `while 1 { }`.
- Code after a `return` is refused rather than silently dropped.
- `let` is immutable and so are parameters; `var` is the mutable binding.
- A binding may shadow one from an enclosing block, but not one in its own.
- An initialiser cannot see the binding it initialises: `let x = x;` reads an
  outer `x` or is refused.
- Functions are not values. M0 has no closures and no function pointers, so a
  bare function name is an error rather than a mystery.

## Things that are scaffolding, and what replaces them

| Scaffold | Why it exists | Replaced by |
|---|---|---|
| `putchar` as a compiler builtin | A program must be able to produce output before there is FFI | M2 made output an effect that must be granted: `putchar` now takes an `&!i Io`. It stays a builtin because `Io` is the console rather than a named library; an `extern fn` reached through `Ffi("libc")` is what replaces it once there are strings to write |
| Wrapping arithmetic on `+`, `-`, `*` | Overflow semantics are still an open decision (#1) | M3: *Defined-behaviour arithmetic* |
| `cc` as the linker | The C runtime supplies `_start` and libc | Much later; "no Rust" is reachable long before "no C" |

Two entries have already left this table. M0 had comparisons yield `int` (`0`
or `1`) and had `if`/`while` test any integer for non-zero, both because it had
no `bool`. M1 introduced one, so a comparison has the type it always meant and
a condition is required to be that type. The convention became a rule, which is
the shape every scaffold above is meant to end in.

Division is *not* on that list. `a / b` and `a % b` trap on a zero divisor and
on `int::MIN / -1`. A trap is defined behaviour; C's answer here is not, and
that difference is the whole point (#1). `tests/` has a fixture that runs the
trap and asserts the process dies rather than continuing with nonsense.

## What the AST already does for canonicalisation

Nothing hashes the AST yet — per-unit identity is M3 work and
`docs/canonical-ast.md` is unwritten. But an AST retrofitted for hashing is an
AST whose hashes are already wrong, so the shape is settled now:

- Nodes live in arenas; **spans live in parallel side tables**. A hash walks
  the arenas and never sees a byte offset.
- Whitespace, comments and redundant parentheses die in the lexer and parser.
  Two files differing only in layout produce identical arenas — there is a test
  that asserts exactly this.
- Integer literals are stored as values, not spellings: `007` and `7` are one
  node.
- Names intern to dense indices, assigned in order of first appearance.
- Every list is a `Vec` in source order. No set, no map, no iteration order
  that depends on an address or a hash seed reaches any output.

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 1 | the program was refused (a located diagnostic was printed) |
| 2 | the command line was wrong |
| 3 | the environment failed: no linker, unwritable output, unsupported host |

`lex-sys run` is the exception: it exits with the compiled program's status, so
`lex-sys run p.ls` and `lex-sys build p.ls && ./p` agree.
