# What a program can reach

> **Status: measured, not argued.**
>
> The question this answers is the one every language gets asked and
> almost nobody answers honestly: *can it do X?* — where X is a REST API,
> a database client, a TLS connection, a thread pool.
>
> The usual answer is a feature list. This document answers by building
> one of the X's and reporting what happened. §1 is the program, §3 is the
> line it ran into, and §5 is the part of the story the effect row cannot
> tell.

---

## 1. A REST endpoint, and it works

`examples/serve/` opens a TCP socket, binds a port, accepts a connection,
routes the request line and answers with JSON:

```
$ lex-sys build examples/serve/serve.ls --std -o serve && ./serve 8080 &
$ curl -i http://127.0.0.1:8080/health
HTTP/1.1 200 OK
Content-Length: 11
Connection: close
Content-Type: application/json

{"ok":true}
```

That is a real socket, a real `accept`, and a real client. The test
harness makes the same request from Rust over loopback and checks the
status line, the body and the declared length
(`an_http_server_written_in_lex_sys_answers_a_real_request`), because a
claim of this shape is worth exactly what it is tested with.

> **And it was worth it.** The first version read the request with a
> single `read` and routed whatever arrived — which is wrong, because one
> `read` returns what has *arrived* rather than what was sent. On an idle
> machine it passed thirty runs in a row; under load it answered 404 to a
> request for `/health` about one run in six. Found while running the
> suite under load for an unrelated reason, and fixed with `read_request`
> (`docs/porting.md` §5). The socket is real, and so are the mistakes a
> real socket lets you make.

**Nothing was added to the language for it.** There is no socket type, no
`Net` capability, no async runtime, no HTTP module in `std`. The program
is 129 lines of code, of which eight are `extern fn` declarations against
libc, and everything else is the slices, regions and capabilities that
were already here.

Under valgrind, answering a live request:

```
total heap usage: 1 allocs, 1 frees, 65,536 bytes allocated
All heap blocks were freed -- no leaks are possible
ERROR SUMMARY: 0 errors from 0 contexts
```

One allocation, and it is the arena's first chunk (`ARENA_CHUNK`). The
request buffer, the response buffer and the `sockaddr` all come out of
it, and it is released by leaving the `region` block. That is not tuning
— it is what a program looks like when the allocation strategy is written
in the source.

### 1.1 Why it works, stated as a rule

> **What decides whether a program is writable is not a feature list. It
> is whether the authority it needs has a name.**

Sockets are libc. libc has a name — `Ffi("libc")` — and that capability
has existed since M2. So the program exists, and it would have existed on
the day the capability did. No feature had to be anticipated, because the
thing that was designed was the *authority*, not the feature.

This cuts both ways, and §5 is the cut.

---

## 2. So the short answer

| Want | Today | Because |
|---|---|---|
| A REST/HTTP server | **Yes** — `examples/serve/` | Sockets are libc, and every argument is a scalar or a byte slice |
| An HTTP client | **Yes** | `connect` is the same shape as `bind` |
| Reading and writing files | **Yes**, without FFI at all | `Fs(prefix)` builtins, narrowable to a directory (`filesystem.md`) |
| A command-line tool | **Yes** | `Args`, `examples/lines.ls`, `examples/wordfreq/` |
| Several processes | **Yes** | `fork` returns an `int`, and an `int` is a value |
| Threads | **No** | `pthread_create` takes a function pointer, and M1 has no function values (§3.3) |
| TLS | **No** | `SSL_CTX *` (§3.1) |
| A Postgres client | **No** | `PGconn *` (§3.1) |
| `malloc`-style allocation | **No**, and it is not wanted | `void *` (§3.1); the heap is a capability with `box` (`heap.md`) |
| Floating-point arithmetic | **No** | There is no `float` type yet. Dated, not structural (§4) |

Read down the "because" column. Three of the four no's are the same
sentence.

---

## 3. What is out of reach, and the one rule underneath it

### 3.1 A pointer does not come back

```
extern fn getenv[&f, &n](ffi: &f Ffi("libc"), name: &n [byte])
    -> [ffi("libc")] &n [byte];
```

```
error: `getenv` returns `&r1 [byte]`, which has no agreed layout across a
foreign boundary; a foreign result is `int` or `bool`, and a C function
that returns nothing is declared `int` and its result discarded
```

A foreign **result** is a scalar. Not because a pointer is hard to
return, but because of what this language promises about the ones it
already has: every reference here carries a region, the region says how
long it is valid, and the checker enforces that. A pointer arriving from
C carries none of that. Accepting one would mean either inventing a
region for it — a lie the checker would then enforce — or admitting a
reference the checker does not track, which is the undefined behaviour
this whole language is built to define away.

So the rule is:

> **A value crosses into a lex-sys program only if the checker can say
> where it came from.**

An `int` qualifies: it is a number, and a number's provenance is nothing.
A `&r [byte]` going *out* qualifies: the region is the caller's and the
callee is handed a pointer and a length that cannot disagree
(`strings.md` §6). A pointer coming *in* does not, and that is the whole
of the list in §2's right-hand column.

Everything that hands back an opaque handle is therefore out: OpenSSL,
libpq, libcurl, `FILE *`, `dlopen`. Not one of them for a reason of its
own.

### 3.1.1 And smuggling one does not help

The declaration is trusted — nothing checks a lex-sys signature against
the C header — so `malloc` can be declared to return `int` and the
compiler will believe it:

```
extern fn malloc[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;
```

That compiles. What it buys is nothing:

```
let p = malloc(f, 64);
let s: &static [byte] = p;     // error: expected `&static [byte]`, found `int`
```

There is no cast, no `from_raw`, no transmute. A reference is produced by
`alloc`, `alloc_slice`, `borrow`, a literal, an index or a subslice, and
every one of those knows its region by construction. **The soundness
boundary is not the FFI check — it is the absence of a raw-pointer
constructor.** The FFI check is there so a programmer meets the rule at
the declaration rather than three hours later.

An integer-shaped pointer can be handed straight back to C, which is how
`fork`'s pid and a file descriptor already work. It just cannot be
dereferenced here.

### 3.2 A struct does not cross either

`bind` wants a `struct sockaddr_in`. The program builds one:

```
let addr = alloc_slice[scratch](16, byte_of(0));
addr[0] = byte_of(2);                                 // AF_INET
addr[2] = byte_of(number / 256);                      // port, network order
addr[3] = byte_of(number - (number / 256) * 256);
```

Four lines, and they are the least defensible four lines in the program:
nothing checks that 16 is the size, that 2 is `AF_INET`, or that the port
is where byte 2 says it is. A `#[repr(C)]` equivalent would check all
three.

It is worth being clear about what this costs and what it does not. It
costs *correctness at the boundary*, which the programmer supplies by
reading a header. It does not cost **safety**: the bytes are a slice, the
slice has a length, `bind` is told that length, and a wrong guess is a
kernel `EINVAL` rather than a read off the end of the buffer. That is the
same trade `strings.md` §6 makes for `write` — the length C is told is
the length the bounds check enforces — applied to a struct instead of a
string.

### 3.3 There are no function values

```
let h = g;
error: `g` is a function; M1 has no function values, so it can only be called
```

A different root, and a smaller one: this is a scope decision, not a
soundness claim. But it is load-bearing for the question people actually
ask, because it is why **threads** are out — `pthread_create` takes a
function pointer, and there is nothing to pass — while `fork` is in, since
it returns an `int` and the child simply continues.

So `examples/serve/` answers one request at a time. A pre-forking server
is expressible today with the same eight declarations plus `fork` and
`waitpid`; a thread pool is not expressible at all.

---

## 4. What was merely missing

Two things this program ran into that were not design at all, only
absence. Both are fixed in the commit that adds it, which is the point of
writing the program rather than reasoning about it.

**`\r` was not an escape.** HTTP's line ending is CRLF. The literal could
not say so, so the first draft assembled `byte_of(13)` into a buffer by
hand — a protocol's own separator, spelled as a number. `strings.md` §4
listed five escapes because nothing had asked for a sixth, which was true
right up until something did. It is a byte with a spelling, not an
encoding claim, so it belongs with `\t` rather than with `\u`.

**The foreign-result refusal named a type the grammar refuses.** It said
*a foreign result is `int`, `bool`, or `()`* — and `tuples.md` §4 keeps
`()` out of the source grammar deliberately, so a reader who took the
advice was told *a tuple has two components or more; there is no `()`*.
Two correct rules and a loop between them. The message now names what can
be written and says what to do about a `void` function.

Neither is interesting. Both were found the same way, and neither would
have been found by reading the documents.

### 4.1 And a third, which this document was credited with naming

`examples/serve/` has four distinct non-zero exits and says nothing on
any of them, because there was no standard error to say it on. Two other
documents — `file-handles.md` §1.2 and `ROADMAP.md` — cite *"`reach.md`
§6's standard-error gap"* as the blocker for their own work.

**There was no such row.** §6 has five and none of them is this; the
second citation was written from the first. A gap two documents think is
recorded, and no document records, is worse than one nobody has noticed:
it looks tracked.

`standard-error.md` is where it is written down, with what the absence
cost measured, and closed — one more label on `Io`, no new capability,
for `standard-input.md` §2's reason. It reached this document's programs
too: `base64` now says `base64: invalid input` where it used to exit 1
in silence. `examples/serve/`'s four remain, because a server's
diagnostics want somewhere to *go*, and that is a question about
logging rather than about a stream.

---

## 5. Where narrowing stops

Here is the price of §1.1, and it is the part worth arguing about.

`examples/serve/` listens on a port. Its authority report says:

```
performs
    args
    ffi("libc")
never touches
    the console
    the filesystem
    the heap
```

**The row does not mention the network, and it is not wrong.** The row is
exact about what the program was granted: it calls a C library. It is
silent about the network because *libc is not an authority domain* — it
is every authority at once. `Ffi("libc")` grants sockets, and it grants
them in the same breath as `abs`.

This is the one place the narrowing story runs out. Everywhere else it is
exact in both directions: `fs_read("/tmp")` is a directory and not the
filesystem, `io_write` is not `io_read`, an absent label is a proof
(`authority.md` §2.2). At the foreign boundary the finest thing that can
be said is a library's name, and a library's name is coarse.

### 5.1 Would a `Net` capability fix it?

**No, and it would make the report worse.** A `Net` capability would sit
next to `Fs` and `Io` and mean *this program may use the network* — and
`examples/serve/` would still not hold one, because it reaches the
network through libc, which is where the network is. Adding `Net` without
taking sockets out of libc produces a program that opens a socket while
declaring no `net`, which is a row that lies.

Taking sockets out of libc means builtins — `socket`, `bind`, `listen`,
`accept` as prelude operations gated by `Net(host)`, the way `filesystem.md` §2
does files. That is a real design and it is the right one *if* the
network is a domain this language wants to mediate itself. It is also
strictly larger than this document, it is the same shape as the
filesystem's, and the argument for it is not "the row should be finer" —
it is "a host is a thing worth narrowing to", which is exactly what
`lex-os`'s perimeter says when it answers *which host?*.

Filed in §6 rather than answered here.

> **Promoted (#74): [`under-a-grant.md`](under-a-grant.md) §5.** The
> argument above frames this as a question of taste — whether a host is
> worth narrowing to — and that framing is now incomplete. Measured
> against `lex-os`'s real grant, **two of its three dimensions cannot be
> decided without it**: a lex-sys program that runs a network server
> reports `args` and `ffi`, so a grant saying `network: None` has nothing
> to refuse it with. The row is the prerequisite for lex-sys code running
> under a grant at all, not a refinement of the report.
>
> **And corrected (#75): [`net.md`](net.md) §1.** *"A host is a thing
> worth narrowing to"* is the **outbound** question, and it describes
> neither side. `examples/serve/` — the only network program here —
> declares `bind`, `listen` and `accept` and **no `connect`**: it is pure
> ingress. A `lex-os` grant's network fields are `network` and `egress`,
> and both are **outbound**; there is no inbound notion in it at all.
>
> Which is right, for a reason that decides the capability's shape:
> outbound is the program's choice and belongs in the type, and inbound
> is not — who reaches a listening port is routing's answer, which is
> why lex-os enforces it with iptables on the tap device rather than in
> the grant. So `Net` has **two axes**: a host outbound, a port inbound,
> and nothing more on the inbound side.

### 5.2 What covers the difference today

The **foreign symbol list**, which exists for a different reason and turns
out to answer this:

```json
{
  "effects": ["args", "ffi"],
  "labels": [{ "name": "ffi", "argument": "libc" }],
  "foreign_symbols": ["accept", "bind", "close", "listen", "read",
                      "setsockopt", "socket", "write"]
}
```

A supervisor reading `socket`, `bind`, `listen`, `accept` knows it is
being asked to run a server, and knows it without trusting a word the
program says about itself: the list is what pass 2 emitted, which is what
`main` reaches, which is what the binary can call (`authority.md` §2).

> **Corrected (#74): [`under-a-grant.md`](under-a-grant.md) §3.** The
> first half of that sentence stands and the second does not. The list is
> a proof about **names** — every foreign call needs a declaration, so it
> is exactly the set reachable from `main` — and a **heuristic** about
> domains, because the step from a name to a domain is the *declaration*,
> and the declaration is the program's to write. Six lines make the point:
>
> ```
> extern fn syscall[&f](ffi: &f Ffi("libc"), n: int, a: int, b: int, c: int)
>     -> [ffi("libc")] int;
> r = syscall(f, 41, 2, 1, 0);        // SYS_socket
> ```
>
> It type-checks, opens a socket, and reports
> `{"effects": ["ffi"], "foreign_symbols": ["syscall"]}`. Against a
> cooperative program the heuristic is worth having. Against the threat
> model `lex-os` exists for, it is not a wall.
The row is a *grant* question and the symbol list is a *what will it
actually do* question, and at the foreign boundary the second is the one
with the answer.

That is a report and not a type, and it should stay a report until §5.1
is answered. A row that said `net` today would be a row that could not be
checked.

---

## 6. Open

| Question | Why it waits |
|---|---|
| A port with resources and depth | `porting.md` §6. `base64` was the first program here that already existed, and it answered this document's §1 from the other side — but it exercised no linearity, no heap and a call graph three deep, so what it shows about *reach* is narrower than it looks |
| Sockets as builtins under `Net(host)` | §5.1. The right shape and a large one: it means taking a domain out of libc and mediating it here, the way `filesystem.md` §2 did for files. The argument turns on whether a host is worth narrowing to, not on the row being finer |
| A checked foreign signature | §3.2. A declaration is trusted against the C header. A generator reading real headers would check it; writing one by hand would not |
| `float` | §2's last row. No design question that is known — it is arithmetic, a Cranelift type and a literal syntax — and it would let `std.math` mean what the name suggests |
| Function values | §3.3. Named as M1 scope rather than refused, and the thing it unlocks first is threads, which wants more than a pointer |

---

## 7. The suite

| Fixture | Rule | § |
|---|---|---|
| `foreign_result_is_a_pointer.ls` | A foreign result is a scalar, and a handle is a pointer | 3.1 |
| `foreign_effect_undeclared.ls` | The row still has to say `ffi` | 1.1 |
| `ffi_without_capability.ls` | And the capability is still the only way in | 1.1 |

| Test | Shows |
|---|---|
| `an_http_server_written_in_lex_sys_answers_a_real_request` | §1, over a real socket, both routes, with the declared length checked against the body |
| `the_authority_report_names_the_syscalls_the_row_cannot` | §5.2: the row is `ffi("libc")`, and the symbols are what say *server* |
| `examples/serve/serve.ls` | The program |
