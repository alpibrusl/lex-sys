# A program that connects

> **Status: a probe, and it found four things.**
>
> [`net.md`](net.md) §5 counted the programs that ask for each half of
> the network and found **inbound 1, outbound 0**. It concluded that the
> next step was not to build `Net`, but to write a program that connects,
> because that is the only way to find out what `connect` needs that the
> design had not thought of. That program is
> [`examples/fetch/`](../examples/fetch/fetch.ls): `curl -s` with one
> method and one protocol version, 335 lines, the same `extern fn`s
> against libc that `examples/serve/` uses. The test suite points it at
> `examples/serve/` itself, so a lex-sys client fetches from a lex-sys
> server.
>
> It found four things `net.md` had not written down. Two of them change
> that design, one confirms a limit that was already known, and one is
> about portability rather than authority. That last one was never on
> the list, and it is smaller than it first looked: the first version of
> §3 was refuted by CI and is corrected in place.

---

## 1. There are no names

```
$ fetch localhost 8080 /health
fetch: the address must be four decimal octets; there is no name resolution
```

Resolving a name means `getaddrinfo`. It answers a `struct addrinfo *`,
and a foreign result is a scalar ([`reach.md`](reach.md) §3.1), so this
language cannot hold what it returns. `fetch`'s whole "resolver" is
`octets_of`: four decimal octets, or a usage error that says why.

`net.md` §6 left this open, with *"§5's program will say which"*
(a builtin, or the perimeter). What the program says is sharper than
either option: **a lex-sys program can only ever connect to an address,
and a `lex-os` grant only ever names hosts**:

```
egress   ["results.demo.internal:443"]      what the grant allows
connect  127.0.0.1:8080                     what the program does
```

A static label could say which host a program means. That can be
compared with the grant by prefix, which is what `net.md` §4 proposed.
A connection only ever says which *address* it reached, and nothing in
the language can relate the two. So whoever resolves the name also owns
the check. There are two candidates, and the difference between them is
the design question `net.md` must now answer:

| Who resolves | What a `net_out` label then names | What checks it |
|---|---|---|
| A builtin (the backend calls `getaddrinfo`) | a host, as the grant does | the builtin, comparing the name before it resolves it |
| The perimeter (`lex-os` already resolves egress to IP rules, its demo's second wall) | an address | the kernel's filter, which is outside the language entirely |

> **Decided (#83): the builtin.** [`net.md`](net.md) §4.1 gives the
> reasons. lex-sys has to be usable without `lex-os`, and only this
> answer gives a standalone program names and a run-time check. `connect`
> checks the name against its capability's bound, then resolves it.
> Under `lex-os`, the firewall stays as an outer wall.

---

## 2. The destination is data

`fetch` takes its address from `argv`. [`net.md`](net.md) §4 wrote the
row as `[net_out("api.example.com:443")]`, with a literal. That fits a
program that always calls one API, but not a client, where the host is
an input. So no row written at compile time can name what `fetch`
connects to.

This is not new. It is `Fs(prefix)`, whose paths are also runtime data:
the capability is narrowed statically to a prefix, and every operation
checks its path against that prefix *at run time* and traps outside it
(`a_path_outside_the_granted_prefix_traps`). A `net_out` label has to
work the same way: a static bound, and a run-time check on each
`connect`. §4's row is still correct, but the literal in it is a
*bound*, not a destination. Stated that way, the check depends on §1:
it compares against a name, or against an address, and §1 has not
decided which.

---

## 3. The address is portable by accident

`struct sockaddr_in` is not the same bytes on the two supported targets:

| | byte 0 | byte 1 | then |
|---|---|---|---|
| Linux | `sin_family` low byte: **2** | `sin_family` high byte: **0** | port (big-endian), four octets, eight zeros |
| macOS | `sin_len`: **16** | `sin_family`: **2** | the same |

The first two bytes contradict each other, and a program with no struct
layout and no target conditionals can write only one of them.
`the_linux_address_layout_connects_on_both_targets` measures what each
target does with each one. It connects to a listener with one layout at
a time, and each CI runner checks its own row:

| | `2, 0` (Linux bytes) | `16, 2` (macOS bytes) |
|---|---|---|
| Linux | connects | **refused**: read as family 528 |
| macOS | **connects**: family 0 read as `AF_INET`, `sin_len` taken from the length argument | connects |

So one array works on both targets: the Linux one. **But it does so
through a compatibility rule, not by being right.** BSD kernels accept
family 0 (`AF_UNSPEC`) as `AF_INET` for old programs that never set it,
and they overwrite `sin_len` with the length the call was given. That
is why `examples/serve/`'s `bind` has always passed on macOS, and it
turns out `connect` follows the same rule.

> **Correction (#77).** This section first said the opposite: that
> *"no one array is right on both"*, that `connect` on macOS would
> refuse the Linux bytes, and that `fetch` had to try both layouts.
> The first two claims came from reading BSD source from memory, not
> from running anything. The darwin-aarch64 runner refuted the
> assertion that encoded them, `fetch` lost its second attempt, and the
> test was renamed to pin what was measured. Linux does refuse the
> macOS bytes; that half was measured here and stands.

What survives is smaller. Both network programs build the struct by
hand, with no layout the language knows. They are portable to exactly
the targets whose kernels forgive the Linux bytes, and a third target
without that rule would break both of them silently, at run time.

---

## 4. Why a connection failed is not observable

When `connect` fails it returns -1, and the reason is in `errno`: a
thread-local that libc reaches through `__errno_location()` on Linux
and `__error()` on macOS. Both return a pointer, so §1's rule applies
again. `fetch` therefore cannot tell *nothing is listening* from
*network unreachable* from *permission denied*. It says `could not
connect` and exits 3.

This is [`reach.md`](reach.md) §3.1's known limit, in the place it
costs most. A network client's error messages are most of what it tells
its user.

---

## 5. What it did not find

- **No need for a socket type.** `net.md` §6 asked whether a descriptor
  wants the linear treatment `File` got. `fetch` has two `close` paths:
  a failed `connect` in `connect_to`, and the end of the exchange in
  `main`. Writing it produced no leak, and a linear type would have
  caught none. The one bug writing it did produce was in buffering: the
  first version copied a whole read into the 4 KiB header buffer, and
  gave up when the end of the header block and the start of the body
  arrived in the same read. `fetch_speaks_http_1_0_and_streams_the_body`
  found it on its first run. That is ordinary code, and no type would
  have prevented it.
- **No need for `Heap`.** The response is streamed: header bytes are
  held until the blank line, and every later byte goes straight to
  standard output. `main` releases `fs` and `heap` on its second and
  third lines.
- **No TLS**, as `net.md` §6 already said. `fetch` speaks HTTP/1.0 in
  plain text.

---

## 6. The count, and what clears the bar

| Half | Programs that ask |
|---|---:|
| Inbound: `bind`, `listen`, `accept` | 1 (`examples/serve/`) |
| Outbound: `connect` | **1** (`examples/fetch/`) |

The bar is two askers ([`standard-library.md`](standard-library.md)),
so `Net` itself is still *settled, not built*. That is the honest result
of adding one program. `the_network_programs_are_counted` pins the count,
so the next network program has to change it.

Both network programs build `struct sockaddr_in` by hand, so there are
two askers for *a socket address the language knows the layout of*.
§3 shows the case is weaker than it first looked. Both programs work on
both targets today, through a BSD compatibility rule. What a builtin
would buy is portability that does not rest on that rule, and a
library cannot provide it, because a library knows no more about the
target than a program does.

It is recorded here, not built, for two reasons. Nothing is broken on
the targets this project supports. And the builtin's shape depends on
§1: an address builtin that takes octets and a port suits the perimeter
answer, and a `connect` builtin that takes a name suits the builtin
answer. Building either before choosing would decide §1 by accident.
*§1 is now decided for the builtin, so it is the one that takes a name
and a port.*

> **Recounted (#171): outbound 2.** `examples/report/` is `fetch/`'s
> shape with a genuinely different HTTP request: `POST <path>` with a
> body, so a `Content-Length` goes out on the connection this time
> rather than only coming back, the way `serve/`'s does. It found
> nothing new about §1 through §4 — the same address rules, the same
> layout, the same opaque `errno`, copied rather than shared because
> there is nowhere between two examples to put a shared function. It
> found two things §1 through §4 had no reason to raise: §8. The
> outbound half has cleared the bar; inbound is still at one.

---

## 7. The suite

| Test | What it pins | § |
|---|---|---|
| `a_lex_sys_client_fetches_from_a_lex_sys_server` | `fetch` against `examples/serve/`, both exchanges: the body, and exit 0 for 200 and 1 for 404 | — |
| `fetch_speaks_http_1_0_and_streams_the_body` | the request byte for byte; a blank line split across two reads; a 100,000-byte body, 25 of the client's reads | 5 |
| `fetch_refuses_a_name_it_cannot_resolve` | a name, three bad addresses and two bad ports are usage errors that say why | 1 |
| `the_linux_address_layout_connects_on_both_targets` | Linux accepts `2, 0` and refuses `16, 2`; macOS accepts both. Each row is measured on its own CI runner. The first version asserted that macOS refuses `2, 0`, and CI refuted it | 3 |
| `the_client_and_the_server_differ_only_in_their_symbols` | both reports are the same unbounded `ffi("libc")`; `connect` is in one symbol list and `bind`/`listen`/`accept` in the other | — |
| `the_network_programs_are_counted` | inbound 1, outbound 2. It used to be `the_only_network_program_is_inbound`, written to fail the day the first outbound program arrived | 6 |
| `a_lex_sys_agent_reports_to_a_lex_sys_server` | `report` against `examples/serve/`: a `POST` gets the same 404 `serve/` gives any unmatched route, over a real connection | 6 |
| `report_sends_a_body_the_server_can_read_in_full` | the request byte for byte, including `Content-Length`, with a 100,000-byte body that takes the client more than one `write` | 6 |

## 8. What changed and what did not

A second outbound program is exactly what §6 says it is: a test of
whether one asker was an accident. It was not, on the design questions
§1 through §4 already settled: `report/` reuses `fetch/`'s address
handling, connect loop and response reader with no changes, and the one
place it differs — assembling a request with a body — is a problem this
project had already solved once, on the other side of the same
connection, in `serve/`'s `respond`. Nothing there is a new design
question, which is itself the finding: the outbound half of `Net`'s
shape generalizes past its first asker.

Writing it found two things anyway, both bugs rather than design
questions, and both only because this program does something `fetch/`
never needed to: send a body large enough to matter.

- **A region is one 64 KiB arena chunk** (`docs/defined-behaviour.md`),
  and `report/`'s first version copied the whole request — headers and
  a message from `argv` — into one scratch slice before sending it, the
  way `fetch/`'s much smaller request is built. A message over roughly
  57 KiB made that allocation itself trap. The fix sends the header
  block and the body as two `send_all` calls instead of one, so the
  body streams straight from the caller's own slice and is never
  copied into an arena at all — the same reason `fetch/` never
  materialises a whole response body either.
- **The header buffer was five bytes short.** Fixing the first bug
  still left `head_out` sized `len(path) + len(host) + 64`: enough for
  the 63 bytes of literal text around it, but with only one byte of
  headroom for `Content-Length`'s digits. Every message this program
  exists to send is large enough that the digit count exceeds that, so
  `put` wrote past the end of the slice and the bounds check caught it
  — the moment a five-figure body was tried, not before. `+ 96` is the
  fix, the same margin `fetch/`'s own scratch buffers already use.

Both were caught by a bounds check and an arena-capacity check, not by
a wrong answer: the failure mode `docs/defined-behaviour.md` promises,
not a silent one. And both are the ordinary cost of a second program
that actually exercises the first one's untested edge, which is the
entire argument for writing it rather than trusting the count.

---

## 9. Slice 1: an address a caller already has

Once both halves cleared §6's bar, `Net` turned out to be bigger than
one slice: `edition N;` parsed but nothing consumed it
([`editions.md`](editions.md) §8), so making edition 2 a real edition
meant threading it through every place a written name resolves — a
function's parameters and return type, a `let` that destructures a
struct, a struct literal, a variant constructor — not only the one
`resolve_type_at` already had a `lookup` closure for. Agreed as three
slices once that came into view: this one (the edition, `Net`, and a
`connect` that takes octets), name resolution, then inbound.

What is built: `Net(bound)`, narrowed the way `Fs(prefix)` is (plain
prefix widening, with no `/`-boundary rule of its own — §4's table has
no boundary character to land on); edition 2's six-field `Split`, a
second declaration of the same source name rather than a sixth field
added to the one all 210 files destructure, because `Split` is `res`
by inference and a field nothing could unname would have to be
consumed by every caller — exactly what
[`editions.md`](editions.md) §7 says an edition must not do; and
`connect(net, a, b, c, d, port)`, which builds the same sixteen-byte
`struct sockaddr_in`, in the same Linux layout §3 measured, that
`examples/fetch/`'s `address` builds by hand — once, in the backend,
so a program with `Net` gets it instead of writing it again.

What is not: the bound is not checked against what is dialled. §4.1's
run-time check compares a *name*, and slice 1 has none to compare —
its row is honestly the bound the capability was narrowed to, exactly
as `open_read`'s row is the directory `Fs` was narrowed to, but nothing
yet traps a `connect` whose octets land outside it. That is slice 2's
`getaddrinfo` call, not a bug in this one: a name is what makes the
check meaningful, and octets alone would only be checking a program
against itself. `examples/fetch/` and `examples/report/` are not
ported onto `connect` for the same reason `net.md`'s status note gives:
both still need `read`, `write` and `close` on the socket `connect`
opens, and those stay `extern fn` against libc (`ffi("libc")`) until a
later slice, so porting now would add `Net` to their report without
removing the wider capability from it.

---

## 10. Slice 2: `getaddrinfo` resolves

`connect`'s shape changes from slice 1's `connect(net, a, b, c, d,
port)` to `connect(net, name, port)`, where `name` is `&r [byte]` — the
one design §1 through §4.1 actually asked for. Octets were a stand-in
for "no resolver exists yet"; `getaddrinfo` accepts a numeric address
exactly as it accepts a symbolic one, so a single name-taking builtin
covers what the four-octet one did and what `fetch/`'s `octets_of`
never could.

### 10.1 The bound is checked before anything is resolved

§4.1 decided this order for a reason: a name outside the bound must
never reach the resolver, or a program could use its narrowed `Net` to
probe DNS for names it was never granted. `connect` takes the name and
the port as two arguments (§4.1's own words), while the bound is one
string, `"host:port"` (§4). The two are split once, at compile time,
since the bound is fixed at that point and neither half needs a
run-time parse: everything up to the last `:` is the host bound,
everything after it the port bound. A bound with no `:` at all —
`""`, unnarrowed, included — has no port bound, which means no
restriction on it.

The host half is `checked_host` (mirroring `checked_path`,
`docs/filesystem.md` §4): it copies `name` into a NUL-terminated stack
buffer while comparing it, byte for byte, against the host bound. Two
things `checked_path` checks that this does not, because neither is a
fact about a host name: the `..`-traversal refusal, and the
requirement that a match land on a separator. `net.md` §4 bounds
`net_out` by plain prefix, with no boundary character of its own —
`narrow`'s own `Net` branch already says so (`lower/mod.rs`) — so
`"api.example.com"` narrows a host bound of `"api"` exactly as far as
the bytes agree and no further rule applies. An unnarrowed `Net` (the
bound is `""`) matches every name, the same way an unnarrowed
`Fs`/`Ffi` matches every path or library.

The port half is different in kind, not degree: `"host:8443"` names
*one* port, not a prefix of ports the way a directory is a prefix of
paths, so the check is equality — the dialled port either is 8443 or
the capability was not narrowed to authorise this call — and it traps
before the host is even copied. A bound with no port restricts
nothing there, the same permissive default the host half has for an
empty bound.

### 10.2 What the resolver is asked for, and what comes back

`getaddrinfo(host, NULL, &hints, &res)`, with `hints` a zeroed `struct
addrinfo` except `ai_family = AF_INET` (2) and `ai_socktype =
SOCK_STREAM` (1) — IPv4 only, matching the one address family this
project has ever built a `struct sockaddr_in` for, and one connection
kind. `service` is `NULL` rather than the port as a decimal string:
building that string is exactly the work `net.md` §6 keeps out of this
language (`std.fmt`'s printer is the one place digits are the language's
problem), and it is unnecessary — the port is two bytes at a fixed
offset in whatever `sockaddr_in` the resolver hands back, the same two
bytes `examples/fetch/`'s `address` writes by hand, and `connect.md`
§3's whole finding was that **only the family bytes differ** between
Linux and macOS. Patching the port after resolving needs no layout
assumption beyond that one, already measured.

`struct addrinfo` is the same 48 bytes, in the same field order, on
every target this project supports — glibc and Darwin's libc both
follow POSIX's `<netdb.h>`, which was standardised after both platforms
existed, unlike `struct sockaddr_in`'s family byte, which predates the
standard that would have settled it:

| Offset | Field | Size |
|---:|---|---:|
| 0 | `ai_flags` | 4 |
| 4 | `ai_family` | 4 |
| 8 | `ai_socktype` | 4 |
| 12 | `ai_protocol` | 4 |
| 16 | `ai_addrlen` | 4 |
| 20 | (padding) | 4 |
| 24 | `ai_addr` | 8 |
| 32 | `ai_canonname` | 8 |
| 40 | `ai_next` | 8 |

Read once, for the first result: `ai_addr` (a `struct sockaddr *`) and
`ai_addrlen`. The backend never reconstructs the address itself here —
unlike slice 1, where there was no resolver to ask — it patches the
port at `ai_addr[2..4]` and passes `ai_addr`/`ai_addrlen` straight to
`connect(2)`, the same call slice 1 already made. `freeaddrinfo(res)`
releases the list; a client connects once, so nothing here needs more
than its first entry.

### 10.3 What this does not do

- **No IPv6.** `ai_family = AF_INET` asks the resolver to filter it
  out, rather than this project deciding what a `Net` bound means for
  two address families that write hosts differently in `host:port`
  text. That question is still open.
- **No port lookup by service name.** `service` is always `NULL`; a
  `Net` bound is `"host:port"`, and this reads the port half itself
  rather than asking the resolver to look up `"http"` or `"https"`.
- **The struct offsets above are asserted, not measured with a
  CI-per-target table the way `struct sockaddr_in`'s were** (§3). They
  follow directly from a documented, versioned interface rather than
  from an implementation this project has already caught disagreeing
  once, so the bar was lower — but `tests/accept/connect_a_refused_address.ls`
  reads through them via a real `connect` on every target this suite
  runs (`accepted_programs_build_and_run`), which is the same
  build-and-run-on-both-runners check §3's table formalised, without a
  table of its own: there is only one row to disagree, not several.
