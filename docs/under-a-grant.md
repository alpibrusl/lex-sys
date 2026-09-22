# Under a grant

> **Status: measured, and it falsifies a sentence in
> [`reach.md`](reach.md) §5.2.**
>
> `README.md` and [`authority.md`](authority.md) both say
> `authority --output json` gives the report *"for a supervisor checking
> it against a grant"*. No supervisor had ever been asked. This takes
> `lex-os`'s real grant and tries to write the check.
>
> One of its three dimensions is enforceable and **lex-sys is finer than
> the grant asks**. Two are not enforceable at all. And the field
> `reach.md` §5.2 offers as covering the difference is a proof about
> *names* and a heuristic about *domains* — six lines defeat it.

---

## 1. The two shapes

`lex-os`'s grant, from `demo/manifest.json`:

```json
"grant": { "filesystem": "Full", "network": "Allowlist", "exec": "Full" },
"egress": ["results.demo.internal:443"]
```

Three dimensions and a host allowlist. A lex-sys authority report, for
`examples/cut/`:

```json
{
  "effects": ["args", "err_write", "heap", "io_read", "io_write"],
  "labels": [{ "name": "args", "argument": null }, ...],
  "foreign_symbols": []
}
```

The question is whether a supervisor holding the first can decide the
second.

---

## 2. One dimension at a time

| Grant dimension | Enforceable from the report? | |
|---|---|---|
| `filesystem` | **Yes, and more precisely than asked** | `fs_read(p)` and `fs_write(p)` carry a *path prefix* where the grant has a tri-state. A report saying `fs_read("/workspace")` answers `ReadOnly` and answers *which directory* |
| `network` | **No** | |
| `exec` | **No** | |
| `egress` — which host | **No information at all** | |

The two no's are one no. Here is `examples/serve/`, which binds a TCP
port, listens, accepts a connection and answers HTTP:

```json
{
  "effects": ["args", "ffi"],
  "labels": [{ "name": "args", "argument": null },
             { "name": "ffi",  "argument": "libc" }]
}
```

**A program that runs a network server reports `args` and `ffi`.** The
row is not wrong — `reach.md` §5 argues that correctly, and the argument
holds: the row is exact about what the program was *granted*, which is a
C library. It is silent about the network because libc is not an
authority domain.

What is new here is the consequence. Against a grant that says
`network: None`, this report is not merely coarse. It is **unable to
refuse**, and the only lex-sys program in this repository that touches
the network is the one it cannot refuse.

---

## 3. The symbol list is a proof about names

[`reach.md`](reach.md) §5.2 offers the foreign symbol list as what
covers the difference, and says a supervisor reading `socket`, `bind`,
`listen`, `accept`

> *"knows it is being asked to run a server, and knows it **without
> trusting a word the program says about itself**."*

The first half is right and the second half is not. Six lines:

```lex-sys
extern fn syscall[&f](ffi: &f Ffi("libc"), n: int, a: int, b: int, c: int)
    -> [ffi("libc")] int;
...
r = syscall(f, 41, 2, 1, 0);        // SYS_socket, AF_INET, SOCK_STREAM
```

It type-checks. Its report:

```json
{ "effects": ["ffi"], "foreign_symbols": ["syscall"] }
```

Neither field says network, and the program opens a socket.

The list is **sound about what it claims**: every foreign call needs a
declaration, so `foreign_symbols` is exactly the set of foreign names
reachable from `main`, and that is a proof in the same way the row is.
The step that is not a proof is the *next* one — from a name to a
domain. `socket` suggests a socket because C's authors named it that,
and the declaration in a lex-sys program is written by the program's
author, who may choose `syscall`, or a wrapper, or a name from a library
whose domains nobody has enumerated.

So: a proof about names, a heuristic about domains. Against a
cooperative program the heuristic is informative and worth having.
Against the threat model `lex-os` exists for — *"the agent is not
trusted"* — a heuristic is not a wall.

§5.2 is corrected in place.

---

## 4. The diagnosis, in one sentence

Every capability in this language bounds what it authorises, except one.

| Capability | Its label says |
|---|---|
| `Fs(p)` | which directory |
| `Io` | which stream — `io_read`, `io_write`, `err_write` are three labels |
| `File` | `file_read`, and the path is spent at `open_read` |
| `Heap` | allocation, which reaches nothing else |
| `Args` | the command line |
| **`Ffi(lib)`** | **which library — and a library is not a domain** |

`reach.md` §5 says this as a limit on *narrowing*. It is larger than
that: `Ffi(lib)` is the one capability whose label does not bound what
it authorises, and every authority domain the language has not yet named
is reachable through it under a single grant.

That is also why the filesystem row above is the one that works.
`filesystem.md` §2 took files **out** of libc and made them builtins
under `Fs(p)`, and the reason the grant's filesystem dimension is
enforceable today is that someone already did the thing §5.1 describes
for sockets.

---

## 5. What this promotes

`reach.md` §6 carries *"sockets as builtins under `Net(host)`"* as an
open row, and §5.1 frames the argument as a question of taste — *"a host
is a thing worth narrowing to"* — with the honest note that adding `Net`
without taking sockets out of libc would produce a row that lies.

That framing was right and is now incomplete. The row is the
**prerequisite for lex-sys code running under a lex-os grant at all**,
because two of that grant's three dimensions cannot be decided without
it. It is not "the row should be finer". It is "the row cannot answer
the question the runtime asks".

The roadmap's `lex-os` join moves accordingly: it is not blocked on a
compiler integration, and the supervisor does not need to embed the
lex-sys front end — `authority --output json` is already the right
interface and `filesystem` already works through it. It is blocked on
the **effect vocabulary**, which has one label where the grant has three
dimensions.

### 5.1 And the report now fails closed (#75)

Until `Net` exists, the report can at least stop being calm about what
it cannot see. An outside audit proposed it and it is the cheapest
correct thing available:

```json
{
  "bounded": false,
  "effects": ["args", "ffi"],
  "labels": [
    { "name": "args", "argument": null,   "bounded": true },
    { "name": "ffi",  "argument": "libc", "bounded": false }
  ],
  ...
}
```

`bounded` is the **first** field, so it is the one a consumer that reads
nothing else reads, and it is `false` whenever any reachable label fails to
name its own domain — which today means exactly `ffi`. The prose report
opens with `UNBOUNDED` and marks the label.

What this does and does not change:

* A supervisor that refuses on `bounded: false` now refuses `examples/serve/`
  under **any** grant, including one where it would have been allowed.
  That is the point of failing closed: the default is safe, and admitting
  a particular unbounded program is a decision about that program, which
  the report should not make on the supervisor's behalf.
* It **does not** make the network enforceable. It makes the absence of
  enforcement impossible to miss. `Net` is still §5's prerequisite.
* The foreign symbol list stays, for the supervisor that does decide to
  read further — §3's heuristic, now clearly labelled as one.

---

## 6. What this does not say

* **Not that the report is broken.** It is exact about what it claims,
  and the filesystem dimension is enforceable *more* precisely than the
  grant can express. What it cannot do, it cannot do for a reason that
  is written down.
* **Not that `foreign_symbols` should go.** It is the best available
  signal and `reach.md` §5.2's first half stands. What changes is the
  sentence claiming it needs no trust.
* **Not a lex-os change.** Nothing here asks that repository for
  anything; the gap is on this side, and so is the fix.
* **Not that `syscall` is a hole to be plugged.** Refusing that one name
  would be theatre — the next spelling is a wrapper. The hole is
  `Ffi(lib)`'s width, not any symbol's name.

---

## 7. The suite

| Test | Shows | § |
|---|---|---|
| `a_network_program_reports_no_network` | `examples/serve/` binds a port and its row is `args`, `ffi` — the gap, pinned, so a future `Net(host)` closes it visibly rather than silently | 2 |
| `the_symbol_list_is_a_proof_about_names` | the six-line `syscall` program type-checks and reports `["syscall"]` | 3 |
| `the_filesystem_dimension_is_enforceable` | a program's `fs_read` prefix is in the report, which is the dimension that works and why | 2, 4 |
