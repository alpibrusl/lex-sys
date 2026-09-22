# What the backend can be asked for, checked against the backend

> **Status: an audit, and it found one thing nobody was looking for.**
>
> Five documents gate the largest row on `ROADMAP.md` — an LLVM backend
> — on four factual claims about Cranelift. Exactly one of them had ever
> been checked against the source: `aliasing.md` §5 read
> `cranelift-codegen` 0.121.2 and found no `noalias`, which is how that
> document reached "two independent blockers" instead of one.
>
> The other three were written from knowledge rather than from reading,
> and a backend that ships every few weeks is a poor thing to know
> about. All four are now checked. **All four hold** — and checking them
> turned up a fifth fact that none of the five documents has:
>
> The two largest unspent facts in this project — a **checked purity
> proof** worth 158× (`purity.md`) and **defined behaviour** costing up
> to 3.35× (`check-cost.md`) — are blocked by two adjacent clauses of
> the same four-line function.

---

## 1. The four claims

| Where | Claim | Holds? |
|---|---|---|
| `aliasing.md` §5 | no `noalias` on a parameter | **yes** (checked in #63) |
| `purity.md` §3 | no "no side effects" attribute on a call | **yes** |
| `gpu.md` §1 | no GPU target | **yes** |
| `gpu.md` §2.3, `check-cost.md` §8 | no SIMD from scalar code | **yes** |

### 1.1 No `noalias`

`ir::AbiParam` is `{ value_type, purpose, extension }`, and `purpose` is
`Normal | StructArgument | StructReturn | VMContext`. The only aliasing
vocabulary in the IR is `MemFlags`' `AliasRegion::{Heap, Table, Vmctx}`
— three fixed regions for WebAssembly's memory model, not a per-pointer
`restrict`.

### 1.2 No purity attribute on a call

`purity.md` says *"there is no call attribute for 'no side effects'"*.
There is not, and the reason is one level lower than a missing field.

`ir::ExtFuncData` is `{ name, signature, colocated }`, and
`ir::Signature` is `{ params, returns, call_conv }` — so there is
nowhere to write one. But even a field would not be read, because
`inst_predicates.rs` decides the question before consulting anything:

```rust
fn trivially_has_side_effects(opcode: Opcode) -> bool {
    opcode.is_call()
        || opcode.is_branch()
        || opcode.is_terminator()
        || opcode.is_return()
        || opcode.can_trap()
        || opcode.other_side_effects()
        || opcode.can_store()
}
```

**`opcode.is_call()`, unconditionally.** A call is side-effecting
because it is a call. Nothing downstream — not the egraph's GVN, not
dead-code elimination — asks anything else about it.

### 1.3 No GPU target

`src/isa/` holds `aarch64`, `pulley_shared`, `riscv64`, `s390x` and
`x64`. `pulley` is Cranelift's own portable interpreter, not a device.
No PTX, no SPIR-V, no AMDGPU, as `gpu.md` §1 said.

### 1.4 No SIMD from scalar code

This is the one worth stating precisely, because "Cranelift emits no
SIMD" is easy to read as a tuning gap.

Cranelift **has** SIMD: vector types, `opts/vector.isle` rewrite rules,
and vector lowerings per target. What it has no pass for is *producing*
them. The optimisation passes in `src/` are `alias_analysis`, `egraph`
(GVN and ISLE rewrites), `legalizer`, `loop_analysis` (a loop tree, for
dominance) and `nan_canonicalization`. There is no auto-vectoriser, and
`loop_analysis` is not one under another name.

So SIMD in Cranelift is an **input language** — it is there because
WebAssembly has `v128` — and a scalar loop stays scalar by design rather
than by oversight. `gpu.md` §2.3 measured lex-sys emitting zero SIMD
instructions and attributed the remaining 2.27× to Cranelift; this is
why that attribution was right, and why it will not improve with a
newer version.

---

## 2. The fifth fact, which is the reason to write this down

Read `trivially_has_side_effects` again with this project's two open
performance arguments in hand:

```rust
opcode.is_call()        // purity.md's 158x
...
opcode.can_trap()       // check-cost.md's 3.35x
```

Asked directly, on the opcodes lex-sys actually emits:

| opcode | `can_trap()` | `is_call()` |
|---|---|---|
| `Trapnz` | **true** | false |
| `Trapz` | **true** | false |
| `Call` | false | **true** |
| `Sdiv` | **true** | false |
| `SaddOverflow` | false | false |
| `Iadd` | false | false |

Three things fall out.

**The two blockers are the same shape.** A pure call cannot be
eliminated because `is_call()` is true; a checked addition cannot be
moved because the `trapnz` beside it has `can_trap()` true. Two
guarantees this language is *proud* of, stopped by two clauses of one
predicate, neither of which takes an argument.

**`sadd_overflow` is pure.** The add-with-flag itself is
side-effect-free — it is the separate `trapnz` that pins it. So the
overflow check's cost is not in the arithmetic at all, which is the
backend's own account of what `check-cost.md` measured from the outside.

**`sdiv` traps by itself**, with no `trapnz` needed. That is
`check-cost.md` §5's finding from the other end: division's guarantee is
free because the instruction already carries it, and the backend agrees
in a boolean.

---

## 3. What follows

`README.md`'s backend row says *"Cranelift for dev, LLVM for release"*,
and until now the case for the second half was a stack of measurements
with an inference on top. It is not an inference any more:

* There is no aliasing fact to emit, so proving `&!` unique buys nothing
  here (`aliasing.md`).
* There is no purity fact to emit, so a 158× stays unspent
  (`purity.md`).
* There is no vectoriser to feed, so removing a trap gets a scalar loop
  (`check-cost.md`, `gpu.md`).

Each of those is a property of the backend's design rather than a
version it has not reached. **The three arguments for LLVM are one
argument**: this language computes facts its backend has no vocabulary
for.

What this does *not* do is make the backend a mistake. Cranelift
compiles fast, it is what a dev loop wants, and `bootstrap.md` chose it
for reasons that still hold. The row says "for dev" and it is right.

---

## 4. What this does not say

* **Not that LLVM would collect all of it.** LLVM has `noalias`,
  `readnone` and a vectoriser, so the vocabulary exists — but
  `check-cost.md` §3 showed the vectoriser only helps where the check's
  condition is per-element, and `poison.md` showed the reduction case is
  not rescued by any spelling. The ceiling is lower than the sum of the
  three numbers.
* **Not a version claim beyond 0.121.2.** Everything here is that
  version, read from `~/.cargo/registry` and, for §2's table, asked of
  the compiled crate rather than inferred from generated source. A
  future Cranelift may grow any of this; the point of writing the
  citations down is that the next person can re-check in minutes.
* **Not an argument for writing our own passes.** `README.md`'s
  non-goals exclude an own optimiser and `purity.md` §3 argues why. A
  middle-end in this repository is the work that turns three months into
  three years.
