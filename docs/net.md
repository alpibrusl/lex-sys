# The network

> **Status: settled, not built.** The program §5 asked for exists now,
> [`examples/fetch/`](../examples/fetch/fetch.ls), and
> [`connect.md`](connect.md) is what it found: two corrections to this
> design, marked below where they apply. The question it opened, who
> turns a host into an address, is decided in §4.1: **the perimeter**.
>
> [`under-a-grant.md`](under-a-grant.md) §5 promoted
> [`reach.md`](reach.md) §6's `Net(host)` row from a question of taste to
> the prerequisite for lex-sys code running under a `lex-os` grant. This
> is the design, written before the code the way `filesystem.md` was —
> and the first thing the probe found is that **§5.1's framing describes
> neither side**.

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

### 4.1 Decided: the perimeter resolves names

[`connect.md`](connect.md) §1 found that a lex-sys program can only ever
connect to an **address**, while a `lex-os` grant only ever names
**hosts**, so whoever turns one into the other also owns the check. There
were two candidates: a builtin that calls `getaddrinfo`, or the
perimeter. **The perimeter resolves. lex-sys never turns a name into an
address.**

The reasons, in the order they decided it:

1. **`lex-os` already resolves, and pins what it resolved.**
   `install_egress_allowlist` in `lex-os-perimeter/src/firecracker/net.rs`
   resolves each egress entry **on the host**, once, when the box is
   provisioned. It takes the first address, installs an `ACCEPT` for that
   IP and port, and ends the chain with a `DROP`. A name that does not
   resolve gets no rule, so the box cannot reach it. That is fail-closed,
   and it is already the enforcement.
2. **Two resolvers disagree.** A builtin resolving inside the box would
   ask a second resolver the same question at a different time. Round-robin
   DNS or a split-horizon zone would give it another address than the one
   the host pinned, and the connection would fail for a reason neither
   side could see.
3. **Resolving in the box widens the box.** The guest would need to reach
   a DNS server, which is egress the grant never listed.
4. **One grant, one enforcer.** `lex-os`'s invariant is that one
   declaration drives every enforcement point. The kernel filter is
   derived from the grant already, and a name check in lex-sys would be
   a second authority over the same traffic.
5. **`getaddrinfo` returns a pointer**, and a foreign result is a scalar
   ([`reach.md`](reach.md) §3.1). The builtin answer would have had to
   make an exception to that rule, and the perimeter answer does not.

What each part now means:

| | Owner | What it checks |
|---|---|---|
| `net_out("host:port")` in a row | the **static** check, before the program runs | that the label is within the grant's `egress`, by name, as §4's table says |
| The address `connect` is given | the **perimeter**, while it runs | that the IP and port are ones it pinned for an allowed host |
| Turning a name into an address | the **perimeter** | nothing in lex-sys does it |

So the future `connect` builtin takes an **address and a port**, never a
name. That also answers the shape question [`connect.md`](connect.md)
§6 left open: the builtin that suits this answer is the one that takes
octets and a port.

**What this costs.** Outside `lex-os`, nothing checks at run time that the
address a program dials belongs to the host its label names. The label
is still checked statically against whatever grant exists, but a program
run on a plain Linux host can connect wherever its `Ffi("libc")` or,
later, its `Net` lets it. That is the same conditionality §3 already
accepts: the guarantee is exactly as strong as the perimeter the
program runs under.

**One thing this hands back to `lex-os`.** A program under this answer
has to dial the address the perimeter pinned, and `resolve_host` keeps
only the first answer. Nothing yet tells the program which one that
was, so a host with several addresses works only when the program is
handed the pinned one. That is a `lex-os` question (how the supervisor
passes a resolved destination in), and the next outbound program will
show what shape it needs.

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

That is the honest state and it is why this document ends at *settled,
not built*. The bar is two and the outbound side has none, so the next
step is not to implement §4 — it is a program that connects, which is
also the only way to find out what `connect` needs that this design has
not thought of. `porting.md` §9 and `line-reading.md` §1 are both about
that being the step nobody can skip.

---

## 6. What this does not do

* **No TLS.** `reach.md` §3.1's rule stands: a foreign result is a
  scalar, so OpenSSL's opaque handles are out regardless of `Net`.
* **No hostname resolution.** `getaddrinfo` returns a pointer. A host in
  a label is a *name to be checked*, and turning it into an address is
  either a builtin of its own or the perimeter's job — §5's program will
  say which. *It did not choose one (#77).* It showed that a lex-sys
  program can only ever connect to an address, while a grant only ever
  names hosts, so whoever resolves also owns the check
  ([`connect.md`](connect.md) §1). *Decided (#PR): the perimeter
  resolves, and lex-sys never does (§4.1).*
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
