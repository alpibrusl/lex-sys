# A program that connects

> **Status: a probe, and it found four things.**
>
> [`net.md`](net.md) §5 counted the programs that ask for each half of
> the network and found **inbound 1, outbound 0**. It concluded that the
> next step was not to build `Net`, but to write a program that connects,
> because that is the only way to find out what `connect` needs that the
> design had not thought of. That program is
> [`examples/fetch/`](../examples/fetch/fetch.ls): `curl -s` with one
> method and one protocol version, 348 lines, the same `extern fn`s
> against libc that `examples/serve/` uses. The test suite points it at
> `examples/serve/` itself, so a lex-sys client fetches from a lex-sys
> server.
>
> It found four things `net.md` had not written down. Two of them change
> that design, one confirms a limit that was already known, and one is
> about portability rather than authority. That last one was never on
> the list.

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

## 3. The address has two layouts

`struct sockaddr_in` is not the same bytes on the two supported targets:

| | byte 0 | byte 1 | then |
|---|---|---|---|
| Linux | `sin_family` low byte: **2** | `sin_family` high byte: **0** | port (big-endian), four octets, eight zeros |
| macOS | `sin_len`: **16** | `sin_family`: **2** | the same |

The first two bytes contradict each other. Linux reads `16, 2` as family
528, and macOS reads `2, 0` as family 0. A program with no struct layout
and no target conditionals cannot write one array that is right on both.
`examples/serve/` writes the Linux bytes and has always passed on macOS.
The reason is BSD compatibility: `bind` accepts family 0 as `AF_INET`,
and the kernel overwrites `sin_len` from the length argument. It is not
because the bytes are right.

`each_target_connects_only_with_its_own_address_layout` measures this
for `connect`. It connects to a listener with one layout at a time, and
asserts that each target accepts its own layout and refuses the other.
On Linux that was measured here: `2, 0` connects and `16, 2` is refused.
The macOS row is measured by the darwin-aarch64 CI runner.

So `fetch`'s `connect_to` tries both, on a fresh socket each time. That
is a program working around its language. It is also the first argument
for socket builtins that has nothing to do with authority: the backend
knows its target, and a program does not. `net.md` §3 argued for
builtins so that `Net` could mean something. This argues for them so
that a network program can be portable at all.

---

## 4. Why a connection failed is not observable

When `connect` fails it returns -1, and the reason is in `errno`: a
thread-local that libc reaches through `__errno_location()` on Linux
and `__error()` on macOS. Both return a pointer, so §1's rule applies
again. `fetch` therefore cannot tell *nothing is listening* from *wrong
address layout* from *network unreachable*. It says `could not connect`
and exits 3, and `connect_to` spends two attempts on a refused
connection because it cannot know that the first failure was not the
layout.

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

One thing did cross the bar, though not the thing `net.md` was about.
Both network programs build `struct sockaddr_in` by hand, and §3 shows
that neither can build it correctly for both targets. That is two askers
for *a target-correct socket address*. It cannot be library code,
because a library knows no more about the target than a program does. So
it is the first network operation with a case for being a builtin that
holds whatever `Net` becomes.

It is recorded here, not built. The builtin's shape depends on §1: an
address builtin that takes octets and a port suits the perimeter answer,
and a `connect` builtin that takes a name suits the builtin answer.
Building either before choosing would decide §1 by accident.

---

## 7. The suite

| Test | What it pins | § |
|---|---|---|
| `a_lex_sys_client_fetches_from_a_lex_sys_server` | `fetch` against `examples/serve/`, both exchanges: the body, and exit 0 for 200 and 1 for 404 | — |
| `fetch_speaks_http_1_0_and_streams_the_body` | the request byte for byte; a blank line split across two reads; a 100,000-byte body, 25 of the client's reads | 5 |
| `fetch_refuses_a_name_it_cannot_resolve` | a name, three bad addresses and two bad ports are usage errors that say why | 1 |
| `each_target_connects_only_with_its_own_address_layout` | Linux accepts `2, 0` and refuses `16, 2`, and macOS the reverse, each measured on its own CI runner | 3 |
| `the_client_and_the_server_differ_only_in_their_symbols` | both reports are the same unbounded `ffi("libc")`; `connect` is in one symbol list and `bind`/`listen`/`accept` in the other | — |
| `the_network_programs_are_counted` | inbound 1, outbound 1. It used to be `the_only_network_program_is_inbound`, written to fail the day this program arrived | 6 |
