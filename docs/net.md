# The network

> **Status: all three slices built.** `Net`, `net_out`/`net_in` and
> `connect`/`bind`/`listen`/`accept` all exist now, behind `edition 2;`
> ([`editions.md`](editions.md) §7). `connect` takes a **name**, not
> octets: it checks the name and the port against the capability's
> bound, then resolves with `getaddrinfo` (§4.1, [`connect.md`](connect.md)
> §10). `bind` takes a **port**, not a name (§2.1, [`listen.md`](listen.md)
> §6.1), checks it against the capability's bound, and folds `socket`,
> `setsockopt(SO_REUSEADDR)` and `bind` into one call; `listen` and
> `accept` take no capability at all, the fd already being what `bind`
> proved the authority for. Split into three slices, agreed when the
> scope came into view mid-implementation: (1) the edition-2 plumbing
> and the capability, (2) `getaddrinfo`-based resolution, closing §4.1's
> "the builtin resolves" the rest of the way, (3) inbound — `bind`,
> `listen`, `accept` and `net_in` ([`listen.md`](listen.md) §6) — all
> three now built. `examples/fetch/` and `examples/report/` are **still
> not** ported onto it: both still need `read`, `write` and `close` on
> the socket `connect` opens, and those stay `extern fn` against libc
> until a later slice, so porting now would add `Net` to their
> capability list without removing `Ffi("libc")` from it — the report
> would read wider, not narrower, which is not what this was for (§3
> already names the day that gap closes). §5.1's framing describes
> neither side, which §1 below found; the question §4.1 opened, who
> turns a host into an address, is decided there: **the builtin, after
> checking the name against its capability's bound**, which is now
> built exactly that way. §5's two-asker bar, per half, is cleared on
> both sides ([`connect.md`](connect.md) §6, [`listen.md`](listen.md)
> §4).
>
> [`under-a-grant.md`](under-a-grant.md) §5 promoted
> [`reach.md`](reach.md) §6's `Net(host)` row from a question of taste to
> the prerequisite for lex-sys code running under a `lex-os` grant. This
> is the design, written before the code the way `filesystem.md` was.

---

## 1. The two directions do not meet

`reach.md` §5.1 puts the argument as *"a host is a thing worth narrowing
to"*. That is the **outbound** question: which host may I reach?

Read what the one network program in this repository actually calls:

```
socket  setsockopt  bind  listen  accept  read  write  close
```

`examples/serve/` is pure **ingress**. It binds a port and waits. There
is no `connect` anywhere in this corpus.

Now read what a `lex-os` grant says about the network — every field, from
every manifest in that repository:

```
network     Full | Allowlist | None
egress      ["results.demo.internal:443"]
```

Two fields, and both are **outbound**. There is no inbound notion in the
grant at all.

> **So the one program lex-sys has does the direction the grant does not
> describe, and the grant describes a direction lex-sys has no program
> for.**

That is not a defect in either. It is the reason this document exists
before the code rather than after it.

---

## 2. Which is right, because the two directions have different owners

The asymmetry is not an oversight in the grant. It is correct, and the
reason is worth stating because it decides the capability's shape.

**Outbound is the program's choice.** A program that calls
`connect("evil.example", 443)` picked that host. The authority question
is *may you pick it*, and the answer belongs in the type, narrowing the
way `Fs(prefix)` does.

**Inbound is not.** A program that binds port 8080 has chosen a *port*.
Who reaches that port is decided by routing, by a firewall, by whoever
holds the network the box is on — **never by the program**. A capability
that claimed to authorise "who may connect to me" would be describing a
decision the program does not make.

`lex-os` already does exactly this, and does it in the right place: its
demo's second wall is **iptables on the tap device**, so the microVM's
NIC has no route to a host outside the allowlist. That is the perimeter
deciding reachability, at a layer the agent cannot touch. The grant has
no inbound field because inbound is not a grant question.

### 2.1 So `Net` has two axes and they are not symmetric

| | What the capability bounds | Who decides the rest |
|---|---|---|
| **Outbound** | *which host* — `Net.out("api.example.com:443")`, narrowing by prefix the way `Fs` does | nobody else; the program picked it |
| **Inbound** | *which port* — `Net.in(8080)` | the **perimeter**, which decides who can reach that port |

An inbound capability that bounded anything more would be a row that
lies, in `reach.md` §5.1's exact sense.

---

## 3. The operations are builtins

The same decision `filesystem.md` §2 made, for the same reason, and it is
the whole point of the exercise:

> **Socket operations reach libc from the backend, the way `putchar`,
> `malloc` and the file operations already do. They are not `extern fn`
> declarations.**

If they stayed `extern fn`, they would be gated by `Ffi("libc")` — and
holding the *FFI* capability would let a program open any socket, with
`Net` contributing nothing. `under-a-grant.md` §4 is that failure already
measured: `Ffi(lib)` is the one capability whose label does not bound what
it authorises, and adding `Net` beside it without taking sockets **out**
of libc leaves both true at once.

Which also means this is not an additive change. A program holding
`Ffi("libc")` can still declare `extern fn socket` and open one, so the
guarantee `Net` offers is *conditional on the grant that program holds*
— and that conditionality is honest, because it is the same one `Fs`
lives with today.

---

## 4. What a row looks like

```
fn fetch[&n](net: &n Net, path: &p [byte])
    -> [net_out("api.example.com:443")] int
fn serve[&n](net: &!n Net) -> [net_in(8080)] int
```

Two labels rather than one, for the reason §2 gives: they answer
different questions and narrow along different axes. A single `net` label
covering both would be back to a library's name — coarse in exactly the
way this document exists to avoid.

Against a grant, the mapping is then direct and is the thing
`under-a-grant.md` §2 could not write:

| Grant | Report | Decidable |
|---|---|---|
| `network: None` | any `net_out` or `net_in` label | **yes** |
| `egress: ["h:p"]` | `net_out("h:p")`, by prefix | **yes** |
| `network: Full` | anything | trivially |

> **Correction (#77).** The literal in `net_out("api.example.com:443")`
> is a **bound**, not a destination. `examples/fetch/` takes its address
> from `argv`, and a client always does, so no compile-time row can name
> where it connects. The shape that works is `Fs(prefix)`'s: a static
> bound, checked against the grant as above, and a run-time check on
> every `connect` that traps outside it. This table is still right about
> the bound. What the run-time check compares against, a name or an
> address, is the question [`connect.md`](connect.md) §1 opens.
> §4.1 answers it.

### 4.1 Decided: the builtin resolves names, after checking them

[`connect.md`](connect.md) §1 found that a lex-sys program can only ever
connect to an **address**, while a `lex-os` grant only ever names
**hosts**, so whoever turns one into the other also owns the check. There
were two candidates: a builtin that calls `getaddrinfo`, or the
perimeter. **The builtin resolves.** `connect` takes a name and a port.
It checks the name against the bound its `Net` capability was narrowed
to, and only then looks it up. A name outside the bound is refused
before any lookup happens.

The deciding requirement is that **lex-sys is usable without `lex-os`**.
Under the other answer, a program on an ordinary host could not use a
host name at all (`fetch` takes four octets today and nothing else), and
nothing would narrow where it connects, since the only check would live
in a perimeter that is not there. The builtin answer gives a
standalone program both: names, and a run-time check against its
capability's bound, in the same shape as `Fs(prefix)`.

**The perimeter answer was considered first**, and its reasons do not
survive the requirement:

| Reason given for the perimeter | Why it does not decide it |
|---|---|
| `lex-os` already resolves each egress host on the host machine and pins the IP (`install_egress_allowlist` in `lex-os-perimeter/src/firecracker/net.rs`) | It keeps doing so. The firewall stays as the **outer** wall, and under `lex-os` a program is checked twice, both times from the same grant |
| Two resolvers can disagree, for example under round-robin or split-horizon DNS | Only if the box asks DNS. The perimeter can write the addresses it pinned into the guest's hosts file. With the usual lookup order (`files` before `dns`), the box then resolves each allowed name to exactly the pinned address |
| Resolving in the box needs DNS egress the grant never listed | Not with the hosts file above: an allowed name resolves locally, and every other name is refused by the bound before a lookup is attempted |
| One grant, one enforcer | Overstated. `lex-os` already has two enforcement points for one grant (its `CLAUDE.md`: *"one declaration, two enforcement points"*). A static check, the builtin's check and the kernel filter are three points derived from one grant, not three authorities |
| `getaddrinfo` returns a pointer, and a foreign result is a scalar ([`reach.md`](reach.md) §3.1) | That rule is about `extern fn` declarations a program writes. A builtin keeps the pointer inside the backend, the way `malloc` already does for `box`, and hands the program an address or an error |

What each part now means:

| | Owner | What it checks |
|---|---|---|
| `net_out("host:port")` in a row | the **static** check, before the program runs | that the label is within the grant's `egress`, by name, as §4's table says |
| The name `connect` is given | the **builtin**, at run time, before resolving | that the name and port are within the capability's bound. It traps or refuses outside it, the way `Fs(prefix)` does |
| The address the name resolves to | the **perimeter**, under `lex-os` only | that the IP and port are ones it pinned for an allowed host |

So the future `connect` builtin takes **a name and a port**, which also
answers the shape question [`connect.md`](connect.md) §6 left open.

**What this hands to `lex-os`.** For the two resolvers to agree by
construction, the perimeter should write its pinned addresses into the
guest's hosts file when it provisions the box. Until it does, a host with
several addresses can resolve differently in the box than on the host.
That is a change in `lex-os`, not here, and nothing in lex-sys depends on
it: a mismatch fails closed, as a refused connection.

**What this does not settle.** The builtin's failure for a name outside
its bound, a trap or a refusal the program can handle, is a question for
the implementation. `Fs(prefix)` traps. A network client may want to
report a bad host as a usage error, as `fetch` reports a bad address
today, so it may want a value instead.

---

## 5. The uncomfortable count

This project's rule is that a feature earns its way in when a program
asks (`standard-library.md`). Counted by reading:

| Half | Programs here that ask |
|---|---:|
| Inbound — `bind`, `listen`, `accept` | **1** (`examples/serve/`) |
| Outbound — `connect` | **0** |

**The half that would unblock the `lex-os` join has no asker**, and the
half with an asker is the one the grant does not ask about.

> **Recounted (#77): inbound 1, outbound 1.** `examples/fetch/` is the
> program the next paragraph asks for. One asker is still below the bar,
> so the status stands. Both network programs build `struct sockaddr_in`
> by hand, and they are portable only through a BSD compatibility rule
> ([`connect.md`](connect.md) §3 and §6).

> **Recounted (#171): outbound 2.** `examples/report/` is a second
> outbound program, an agent that posts a result rather than a generic
> client, so it exercises a shape `fetch/` does not: a request with a
> body, needing a `Content-Length` going out the same way `serve/`'s
> responses need one coming back. **The outbound half has cleared the
> bar.** Inbound is still at one, so the two halves are no longer
> together: building `connect`/`net_out` is now justified by this
> project's own rule, and building `bind`/`listen`/`accept`/`net_in`
> still is not, until a second inbound program arrives.

> **Recounted (#176): inbound 2.** `examples/collect/` is a second
> inbound program ([`listen.md`](listen.md)): it accepts more than one
> connection without restarting and reads a request's body, the two
> things `serve/` never had to do. **Both halves have now cleared the
> bar.**

> **Recounted (#122): outbound 3.** `examples/vsock/` connects over
> `AF_VSOCK` -- the channel `lex-os-guest` uses to reach its host
> supervisor, scoping the first slice of moving `lex-os` off Rust. It
> is `extern fn socket`/`connect`/`close` again, the same as `fetch/`
> and `report/`, because `AF_VSOCK` is Linux-specific and has no `Net`
> builtin behind it -- and `docs/reach.md`'s own point stands: it does
> not need one, `Ffi("libc")` already names the authority. Not a
> change to the bar itself, just a third program clearing it the same
> way.

That is the honest state up to here. Both halves of the two-asker bar
§5 set are cleared, and §3's own reason to build now rather than before
still applies in full: `Ffi("libc")` lets a program declare
`extern fn socket` and `extern fn connect` on its own, so `connect`
becoming a builtin does not by itself take sockets out of libc — a
program that also holds `Ffi("libc")` (as `examples/fetch/` and
`examples/report/` still do, for `read`/`write`/`close`) contributes
nothing new by adding `Net` beside it, which is exactly why those two
programs are not yet ported onto it (top of this document). `Net` is a
real, narrower way to open a connection, and it is honest only once
holding it — and not `Ffi("libc")` — is enough to speak the network,
which needs the remaining socket operations to become builtins too.

---

## 6. What this does not do

* **No TLS.** `reach.md` §3.1's rule stands: a foreign result is a
  scalar, so OpenSSL's opaque handles are out regardless of `Net`.
* **Hostname resolution: built.** `getaddrinfo` returns a pointer, so
  it stays inside the backend the way `malloc` already does for `box` —
  a program never sees it, only the address or the failure. A host in a
  label is a *name to be checked*, and turning it into an address is
  either a builtin of its own or the perimeter's job — §5's program
  will say which. *It did not choose one (#77).* It showed that a
  lex-sys program can only ever connect to an address, while a grant
  only ever names hosts, so whoever resolves also owns the check
  ([`connect.md`](connect.md) §1). *Decided (#83): a `connect`
  builtin resolves, after checking the name against its capability's
  bound, and the perimeter stays the outer wall under `lex-os` (§4.1).*
  Slice 1 built `connect` for an address a caller already had, the
  `octets_of` half of `examples/fetch/` moved into the backend
  (`docs/connect.md` §1); slice 2 replaced it with a `connect` that
  takes a name and resolves it (`docs/connect.md` §10) — the shape §4.1
  actually decided, now built exactly that way.
* **No socket type.** A descriptor is an `int`, as `File` was before
  `file-handles.md` gave it a linear type. Whether a socket wants the
  same treatment is a question that program answers too. *It answered
  no, for now (#77):* two `close` paths, no leak, and a linear type
  would have caught nothing ([`connect.md`](connect.md) §5).
* **Nothing about `exec`.** The grant's third dimension has the same
  shape as this one and none of the same operations. It waits for its own
  document.

---

## 7. The suite

| Test | Shows | § |
|---|---|---|
| `the_network_programs_are_counted` | The count §5 rests on, file by file: inbound `examples/serve/`, outbound `examples/fetch/`. It was `the_only_network_program_is_inbound`, written to fail when an outbound program landed. It did, and §5 was recounted rather than left to age | 1, 5 |
