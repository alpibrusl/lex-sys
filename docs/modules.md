# Modules

> **Status: settled.** The precondition for a *standard* library, and the
> first thing here that changes how a name is resolved.

---

## 1. What asked for it

`many-files.md` unblocked a library: a program is the set of files named
on the command line, sharing **one flat namespace** — no `import`, no
namespaces, no visibility, "because each of those is a design and the
minimum that unblocks a library is none of them".

That was right for a library. It is not enough for a *standard* one.

The evidence is countable. Across this repository's examples and accept
fixtures, `print_nat` is defined **25 times**, byte for byte identically,
and `write_all` **19 times**. Every program rewrites them because there
is nowhere to put them — and putting them in one flat namespace would
mean a standard library owning names like `open`, `push` and `total`
that programs here already use for their own things.

So: a namespace, a way to reach into one, and a way to keep something
out of reach.

---

## 2. A module is a namespace, not an identity

This is the constraint that shapes everything below, and it comes from
`canonical-ast.md` §1:

> *"moving a function between files changes nothing about it."*

A module must not break that. So a module name is **not** part of any
hash: not a `SigId`, not a `BodyId`, not a type's identity.

The surprising part is that callers do not change either. A call already
encodes the callee's **hash** rather than its spelling — that has been
true since M0, for an unrelated reason (a callee's body may be rewritten
without touching a caller's hash). So moving `print_nat` into `std.io`
and calling it as `io.print_nat` changes:

* not `print_nat`'s own `SigId` or `BodyId`, because the module is not in
  them, and
* not the caller's `BodyId`, because the call encodes a hash that did not
  move.

**Modules cost the identity system exactly nothing**, and that is a
property of content-addressing rather than luck: in a language that
addresses code by what it is, a namespace is a thing the *reader* needs
and the store does not. `moving_a_function_into_a_module_changes_no_hash`
is that claim as a test.

---

## 3. Declaring one

```
module std.io;
```

The first item in a file, and at most one. A file with no `module`
declaration is in the **root** module — so every program written before
this document still compiles, unchanged, which is the whole compatibility
story and it costs nothing.

### 3.1 Declared, not derived from the path

A module could be named by where its file sits — `std/io.ls` is
`std.io`. It is not, because `many-files.md` §3 chose identity by content
rather than location, and deriving a module name from a file path puts
the file system back into the language: a build that reorganises
directories would rename modules, and a hash that does not depend on
location would sit inside a name that does.

### 3.2 Two files may declare the same module

They share its namespace, exactly as two files in the root share the root
today. A module is a name, not a file.

---

## 4. Importing one

```
import std.io;              // bound as `io` — the last segment
import std.io as console;   // bound as `console`
```

Per **module**, and the binding is a **qualifier**, not a set of names:

```
io.print_nat(i, 42);
let b: io.Buffer = io.empty();
```

Per module rather than per file, because §3.2 already says two files
declaring one module share its namespace — and a namespace you share
while each half sees different names is two namespaces wearing one name.
So a module's imports are the union of what its files import, and a file
that imports nothing still sees what its module's other file imported.

### 4.1 Why qualification is required

An unqualified import — bring `print_nat` itself into scope — puts back
the collision that modules exist to prevent, and this language has no
visibility-scoped shadowing story to resolve one with. §7 keeps it open;
the minimum that unblocks a standard library is the qualified form.

### 4.2 Cycles are fine

A module may import one that imports it back. In most languages that is a
problem because imports drive load order; here the program is the set of
files on the command line, compiled at once, and an import is a rule for
resolving a name rather than an instruction to go and read something.
Functions have been mutually visible since `many-files.md`; modules do
not change that, they only decide what a name is spelled.

---

## 5. Visibility

`pub` on a function, struct or enum. Default **private to its module**.

```
pub fn print_nat[&i](io: &!i Io, n: int) -> [io_write] int { ... }
fn digit(n: int) -> [] int { ... }          // private
```

Private means invisible from another module — including in a type. A
`pub fn` whose signature mentions a private type is refused, because a
caller that cannot name the type cannot call the function, and a
signature that cannot be used is a signature that is wrong.

Within the root module everything is visible to the root, which is why
existing programs need no `pub` anywhere.

### 5.1 The root module cannot be imported

It has no name, so there is nothing to write. Once a program uses
modules, `main` lives in the root and imports what it needs; the root
sees out and nothing sees in.

---

## 6. What a module is **not**

* **Not a trust boundary.** This is the one that matters. Authority here
  comes from `split(world)` and from nowhere else (§8.2 of
  `linearity-and-effects.md`), and a module cannot grant, widen or
  conjure a capability. `pub` means *reachable*, never *safe*: a `pub fn`
  that takes an `&!i Io` still needs a caller holding one, and its row
  still says what it did. A module boundary hides names. It does not
  hide effects, and it must never be read as though it does.
* **Not a compilation unit.** The program is still the set of files on
  the command line. Modules do not add a build step, a search path or a
  manifest.
* **Not a package.** No versions, no fetching, no registry. That is a
  distribution question and this is a naming one.
* **Not a file path.** §3.1.

---

## 7. Open

| Question | Why it waits |
|---|---|
| Unqualified import — `import std.io.print_nat` | §4.1. It needs a collision rule, which is a design |
| Re-export | Useful for a `std` that is many files behind one name; needs §4.1 first |
| A package and version story | §6. Distribution, not naming |
| Importing the root | §5.1. It would need a name, and naming the anonymous thing is its own decision |

---

## 8. The suite

Most of these need **two files**, and `tests/reject/` compiles one at a
time — so they live in the conformance harness instead, under
`the_module_rules_are_enforced_across_files`. Same discipline: every rule
here has a program that breaks it, and the message it is refused with is
written down.

| Rule | Where | § |
|---|---|---|
| A `module` declaration is the first item | `tests/reject/module_not_first.ls` | 3 |
| At most one per file | `tests/reject/two_module_declarations.ls` | 3 |
| An import names a module the program declares | `tests/reject/import_unknown_module.ls` | 4 |
| Two imports may not bind the same qualifier | conformance, with the `as` that fixes it | 4 |
| Private is private, for a function | conformance | 5 |
| Private is private, for a type | conformance | 5 |
| A `pub` signature names usable types | `private_type_in_a_pub_signature` | 5 |
| An import binds a qualifier, not a set of names | conformance | 4.1 |
| A qualified name that is not there is an error, not a fallback | conformance | 4 |

| Accepting | Shows |
|---|---|
| `examples/modular/` | Two modules and a root: a qualifier, an `as`, a private helper, a module importing another, and a qualified type in an annotation and a pattern |

And four that are not about refusing anything:

* `moving_a_function_into_a_module_changes_no_hash` — §2, the claim that
  modules are free, checked rather than asserted.
* `two_modules_declaring_the_same_function_agree_on_its_hash` — the other
  side of §2. Identical code has one identity however many namespaces
  mention it, which is content-addressing working rather than a
  collision.
* `pub_grants_no_authority_and_hides_no_effect` — §6. Worth a test rather
  than a sentence, because "public API" means *sanctioned* in most
  languages and here it must not.
* `modules_may_import_each_other` — §4.2.
