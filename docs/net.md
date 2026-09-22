# The network

> **Status: settled, not built.** The program §5 asked for exists now,
> [`examples/fetch/`](../examples/fetch/fetch.ls), and
> [`connect.md`](connect.md) is what it found: two corrections to this
> design, marked below where they apply.
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
> so the status stands. What did cross the bar is narrower: both network
> programs build `struct sockaddr_in` by hand, and neither can build it
> correctly for both targets ([`connect.md`](connect.md) §3 and §6).

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
  ([`connect.md`](connect.md) §1).
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
