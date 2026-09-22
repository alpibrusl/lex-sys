# When the compiler is wrong

> **Status: settled and built.**
>
> The audit's C2(c): *"catch Cranelift verifier errors and report an
> `internal` rule with the span of the function being lowered, never a
> bare panic."* This document measures what a backend failure gives a
> reader today. It is worse than the audit described in one way the
> audit did not name: **`check` passes the program.**

---

## 1. What a backend failure gives today

[`emitted-checks.md`](emitted-checks.md) §1 found a real one: a folded
`byte`-returning call that the Cranelift verifier refused. #71 fixed it,
and for this measurement it was put back for one build and then
restored. The program that hit it:

```
fn g(n: int) -> [] byte { return byte_of(n); }
fn main(world: World) -> [] int {
    release(world);
    let t = int_of(g(65));
    return t - 65;
}
```

| Command | Answer | Exit |
|---|---|---|
| `lex-sys check` | nothing | **0** |
| `lex-sys check --output json` | `{ "refused": [] }` | **0** |
| `lex-sys build` | ``code generation failed: in `main`: Compilation error: Verifier errors`` | **3** |

Three things are wrong with that, and only the first is the one the
audit named:

1. **No rule and no position.** The one piece of structure is the
   function's name, inside the prose.
2. **Exit 3 is the wrong category.** The CLI's own table documents 3 as
   *"the environment failed: no linker, unwritable output, unsupported
   host."* A compiler bug is reported as the machine's fault. An agent
   that branches on exit codes checks its toolchain, not its program.
3. **`check` says the program is fine.** `check` stops after lowering and
   never runs the backend, so the tool [`agent-errors.md`](agent-errors.md)
   built for machines answers an empty list for a program that will not
   build. An agent loop of *check, then build* passes its gate and then
   fails in a step it was told was only about linking.

There is a second way the backend fails, and it is found by reading the
code, not by running it. `lex-sys-codegen` has **22** `unreachable!`,
`panic!` or `.expect(` sites outside its tests. Nearly all of them say
the same thing: *"the checker should have refused it."* Each one is an
invariant between the front end and the back end, and if one ever
breaks, the reader gets a Rust panic message and exit **101**. That is
not even one of the four documented codes.

---

## 2. The rule

A backend failure is a refusal with rule **`internal`**:

```
sort.ls:12:1: error: the compiler failed to generate code for `merge`; this is a bug in lex-sys, not in the program (Compilation error: Verifier errors)
```

```json
{
  "rule": "internal",
  "message": "the compiler failed to generate code for `merge`; this is a bug in lex-sys, not in the program (Compilation error: Verifier errors)",
  "explanation": "The compiler failed on a program it had accepted. The program is not at fault; the position is the function whose code could not be generated.",
  "position": { "file": "sort.ls", "line": 12, "column": 1 }
}
```

- **Where it points.** At the declaration of the function whose code
  failed. That is the smallest unit the backend works on, and the one a
  reader can act on: rewrite that function to avoid the construct, and
  report the bug. A generic instance points at its generic declaration.
  A failure with no function (defining a data object, finishing the
  module) has no position, as [`agent-errors.md`](agent-errors.md) §5
  allows, and none is invented.
- **The cause stays in the message.** Cranelift's own text follows in
  parentheses. It is what a bug report needs, and dropping it would make
  the rule less useful than the prose it replaces.
- **Exit 1, not a new code.** The program is refused: it cannot be
  built, whoever is at fault. A consumer that already reads
  `check --output json` handles this with no change, and tells it apart
  by the tag. A fifth exit code would change the contract that every
  consumer branches on, to say something the tag already says.
- **The tag is stable** ([`agent-errors.md`](agent-errors.md) §3.1). It
  names a category that should stay empty. A program that reaches it
  has found a bug.

---

## 3. `check` runs the backend

A program `check` accepts should build, and the only reasons left for
`build` to fail should be the environment's: the linker, a disk. So
`check` now generates the object code too, and throws it away.

Measured on this machine, median of seven runs:

| Program | `check` before | `check` now | Ratio |
|---|---:|---:|---:|
| `examples/sort/` | 4.8 ms | 11.0 ms | 2.3× |
| `examples/base64/` | 4.2 ms | 8.3 ms | 2.0× |
| `examples/fetch/` | 4.2 ms | 8.6 ms | 2.0× |
| `examples/tour.ls` | 5.4 ms | 12.6 ms | 2.3× |
| the differential's 9,500-function program | 222 ms | 570 ms | 2.6× |

About twice the time, and a few milliseconds in absolute terms on
every real program here. That is the price of `check` being a promise
and not a guess. `authority`, `ids` and `layout` do not run the
backend, because what they report is about the checked program, and a
backend failure would still stop `build`.

---

## 4. A panic is caught at the function

Each function's code generation runs under `catch_unwind`. A panic
becomes the same `internal` refusal, located at the function that was
being generated, with the panic's own message as the cause. While the
backend runs, the default panic hook, which would print Rust's
*"thread 'main' panicked at"*, is replaced by a silent one and then
restored. The reader sees a located refusal, not a backtrace hint.

This is a boundary, not error handling. Code generation for that
program stops at the first failure; there is no partial object. What
changes is only what the reader is told.

---

## 5. How it is tested without a bug to test it on

Nothing here fails today, which is the point. So the tests build what
the front end never produces: an IR function whose declared return type
disagrees with what its body returns. That is #71's bug in its smallest
form, written straight into the IR:

- `a_backend_failure_names_its_function` (in `lex-sys-codegen`) checks
  that the verifier's refusal comes back naming the function that
  failed.
- `a_backend_failure_is_a_located_internal_refusal` (in the CLI) takes a
  real checked program, breaks one function's IR, and checks the
  refusal: rule `internal`, positioned at that function's declaration,
  and saying it is the compiler's bug.
- `a_backend_panic_is_a_located_internal_refusal` does the same for a
  panic, by handing the backend a node the checker would have refused.
- `every_rule_has_a_fixture` lists `internal` as covered elsewhere,
  beside `not-public`, because no source file can reach it on purpose.

---

## 6. What this does not do

- **It does not find the bugs.** That is C2(a), fuzzing the parser and
  the checker. This makes what fuzzing finds reportable, and it gives a
  fuzzer a tag to count.
- **It does not change a single existing refusal.** The 52 rules, their
  messages and their fixtures are byte for byte what they were
  (`the_prose_is_unchanged_by_the_json`).
- **It does not put spans in the IR.** Each function carries the span
  of its declaration and nothing finer. [`compile-time.md`](compile-time.md)
  §9's reason for keeping expressions spanless still holds: a
  folded call that traps is left alone to trap at run time.
