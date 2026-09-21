# `[budget]`

> **Status: settled, and the answer is that it does not belong here.**
>
> §12 lists it as carried over from Lex and unspecified: *"plainly a
> capability carrying an integer; what it costs at runtime, and whether
> it is checked or merely accounted, is unanswered."*
>
> The epic mentions it once — *"Carried from Lex: examples-as-tests,
> `[budget]`, effect declarations as contract"* — and nowhere else. It
> is a name, not a design. This settles what it would have to mean, and
> why the thing it means is already built somewhere else.

---

## 1. Three questions, not one

A capability language answers *may this program reach X?* A budget
answers *how much may it consume?* Those are different questions, and
`lex-os` — the first real target for this language — already separates
them into three layers:

| Layer | Question | Where |
|---|---|---|
| Type check | May this reach the network / the filesystem **at all**? | `lex-os-check`, against the manifest's `Grant` |
| Perimeter | **Which** host, which path? | The kernel firewall and the sandbox policy |
| Budget | **How much**? | The supervisor, charged per mediated command |

That is not a reading of mine; `lex-os-check`'s own header says it:
*"The type-check answers 'may this box touch the network at all?'; the
perimeter answers 'which host?'. Two layers, one grant."* The budget is
the third, and it is charged in `lex-os-supervisor`'s mediation loop —
log → reversibility → perimeter → **budget → charge** → allow.

`lex-sys` owns the first layer, and owns it well: the effect row *is*
the answer, `lex-sys authority` reports it, and `docs/authority.md` §2
shows it falls out of reachability the compiler already computes.

## 2. The units settle it

`lex-os`'s `Budget` is:

```rust
pub struct Budget {
    pub wall_clock_secs: u64,
    pub max_commands: u64,
    pub max_money_cents: u64,
    pub max_api_calls: u64,
}
```

**None of those is a property of a program's text.** Wall-clock seconds
depend on the machine; money in cents depends on what an API charges;
commands and API calls are counted by the thing mediating them. A type
system reads source, and no amount of reading source tells you how many
cents an action will cost.

So a `[budget]` row could only ever carry a *proxy* — instruction
counts, call counts, allocation counts — and a proxy for the thing you
care about, checked in units nobody wrote the limit in, is worse than
no answer.

## 3. And both implementations conflict with a stated commitment

**Dynamic fuel** — a counter decremented per operation, trapping at
zero — is a hidden per-operation cost. The epic's performance
expectation says the *one* structural cost this language accepts is
defining away UB, at "low single-digit percent". A fuel counter is a
second one, larger, and paid by every program including the ones that
never asked for a budget.

**A static bound** — `-> [budget(500)] T`, summed by the compiler —
needs a cost model. The epic commits to *"Cranelift (dev) → LLVM
(release)"*: two backends, different generated code, different real
costs. A bound in machine units cannot be stable across them, and a
bound in abstract units is arbitrary — a number that means nothing in
particular, checked exactly.

## 4. What a program does today, and why it is enough

`examples/pipeline.ls` runs jobs against a budget right now, with no
language support:

```
fn admit[&t](job: Job, tally: &!t Tally, budget: int) -> [] int {
    if tally.spent + cost <= budget { ... }
}
```

An integer, threaded, checked where the program decides an action costs
something. That is **policy**, and this language has put policy in
programs every time the question has come up: no `realloc`, so the
doubling policy is `std.buffer`'s; no destructor, so ending a resource
is the owner's; no generic `drop`, so `collections.md` §4 leaves ending
a `T` to whoever knows what it means.

A budget is the same shape of decision. What counts as an "operation",
what it costs, and what happens at zero are all things the program knows
and the language does not.

### 4.1 The one thing a capability would add

Unforgeability. Any program can write `Tally { spent: 0 }`; nothing can
conjure an `Io`. So a `Budget` capability would mean *you cannot grant
yourself more*, which is a real property and the reason `Io` is a
capability rather than a struct.

It fails on where the number comes from. `split(world)` would have to
hand out a `Budget(n)`, and the runtime does not know `n` — it would
have to come from an environment variable or the command line, which is
the language making a policy choice, or be unlimited, in which case it
is not a budget.

The number comes from **whoever is doing the budgeting**, and that is
not the program. It is the embedder — which for Lex is its host, and for
`lex-sys` is `lex-os`, where the number is in a signed manifest and the
charge happens before the effect runs.

---

## 5. So the answer is no, and here is what was wanted instead

The useful half of the request is not enforcement, it is **legibility**:
a supervisor deciding whether to run a program wants to know what the
program can reach, in a form it can check against a grant.

That is `lex-sys authority`, and this slice gives it a machine-readable
form:

```sh
$ lex-sys authority examples/tour.ls --std --output json
{
  "effects": ["args", "ffi", "fs_read", "fs_write", "heap", "io_write"],
  "labels": [
    { "name": "args", "argument": null },
    { "name": "ffi", "argument": "libc" },
    { "name": "fs_read", "argument": "/tmp" },
    ...
  ],
  "foreign_symbols": ["labs"]
}
```

The shape is deliberate: `effects` is the distinct kinds, which is the
coarse question a grant is written against, and `labels` keeps the
narrowing for the precise one. `lex-os-check`'s `CheckReport` draws
exactly that line for Lex programs —

```rust
pub struct CheckReport {
    pub effects: Vec<String>,
    pub net_hosts: Vec<String>,
}
```

— so a `lex-sys` program can be checked against a manifest the same way,
by the same wall, without `lex-sys` learning what a cent is.

---

## 6. Open

| Question | Why it waits |
|---|---|
| Wiring this into `lex-os-check` | A change in that repository, against a `Grant` whose vocabulary is its own. §5 is the half `lex-sys` owes |
| An `--output json` for `check` | Diagnostics as data, which is a separate and larger surface than one report |
| A cost model, if a static bound is ever wanted | §3. It needs one backend, or a unit that is honest about being abstract |
