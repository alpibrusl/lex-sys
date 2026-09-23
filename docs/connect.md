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

> **Decided (#83): the perimeter.** [`net.md`](net.md) §4.1 gives the
> reasons. The label still names a host, and it is checked against the
> grant statically. Only the perimeter turns names into addresses, and
> it enforces the address at run time.

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
*§1 is now decided for the perimeter, so the builtin is the one that
takes octets and a port.*

---

## 7. The suite

| Test | What it pins | § |
|---|---|---|
| `a_lex_sys_client_fetches_from_a_lex_sys_server` | `fetch` against `examples/serve/`, both exchanges: the body, and exit 0 for 200 and 1 for 404 | — |
| `fetch_speaks_http_1_0_and_streams_the_body` | the request byte for byte; a blank line split across two reads; a 100,000-byte body, 25 of the client's reads | 5 |
| `fetch_refuses_a_name_it_cannot_resolve` | a name, three bad addresses and two bad ports are usage errors that say why | 1 |
| `the_linux_address_layout_connects_on_both_targets` | Linux accepts `2, 0` and refuses `16, 2`; macOS accepts both. Each row is measured on its own CI runner. The first version asserted that macOS refuses `2, 0`, and CI refuted it | 3 |
| `the_client_and_the_server_differ_only_in_their_symbols` | both reports are the same unbounded `ffi("libc")`; `connect` is in one symbol list and `bind`/`listen`/`accept` in the other | — |
| `the_network_programs_are_counted` | inbound 1, outbound 1. It used to be `the_only_network_program_is_inbound`, written to fail the day this program arrived | 6 |
