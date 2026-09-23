# A second inbound program

> **Status: probe done, slice 3 built.** `net.md` §5 and `connect.md`
> §6 count inbound and outbound separately, each against a bar of two
> askers; §4 below is where this probe cleared inbound's. `Net`'s
> third slice ([`connect.md`](connect.md) §9's plan) builds on that
> count: `bind(net, port)`, `listen(fd, backlog)` and `accept(fd)`, and
> the `net_in` label, all behind `edition 2;`. §6 is the build.

---

## 1. What `serve/` never had to do

`examples/serve/` binds one port, accepts **one** connection, answers
**one** request, and exits — which `reach.md` and `net.md` §5 both note
is what makes it testable without a shutdown protocol. Its router reads
only the request line (`read_request` stops at the first `\n`); nothing
in it ever reads a body, because `GET /health` has none.

That leaves two things a second inbound program can exercise that the
first one never did, and both are ordinary parts of being a server
rather than exotic ones: accepting **more than one** connection without
restarting the process, and reading a request that **carries a body**.

## 2. `collect/`: a bounded accept loop that reads one

`examples/collect/collect.ls` takes a port and a count on the command
line, accepts that many connections in a loop, and for each one reads
a `POST` request in full — headers, then the body, by `Content-Length`,
the same accounting `examples/report/`'s `exchange` sends by
(`connect.md` §8) — and writes the body to standard output before
moving on to the next connection. It answers every request with a
bare `200`, since the point is what it can read, not what it can route.

A **fixed count**, taken from `argv`, is the same discipline
`examples/serve/`'s own test already relies on: a server with no
shutdown protocol is testable only if something bounds how long it
runs, and "one" was `serve/`'s bound. `collect/`'s is a number instead
of a constant one, which is the whole of what makes the loop a loop
rather than a copy of `serve/` with a bigger number hard-coded in.

## 3. What this does and does not settle

This is not a new design question the way `connect.md` §1–§4 were.
`net.md` §2 already decided inbound's shape — a capability bounds a
*port*, and who may reach it is the perimeter's answer, not the
program's — and nothing about reading more than one request or a body
changes that. `collect/` is here to clear `connect.md` §6's count, the
same way `examples/report/` did for outbound, and to find what a second
inbound program finds by being written rather than assumed.

## 4. The count

| Half | Programs that ask | ago |
|---|---:|---|
| Inbound: `bind`, `listen`, `accept` | 1 (`examples/serve/`) | `net.md` §5, `connect.md` §6 |
| Inbound, recounted | **2** (`examples/serve/`, `examples/collect/`) | here |

Both halves of `Net` have now cleared the two-asker bar.

## 5. What writing it found

Not in `collect.ls` itself: reused unchanged from `docs/connect.md` §8's
answer, its body streams straight to standard output rather than being
copied into a scratch slice, so a region's 64 KiB arena cap is never in
play here either.

**In the test.** `collect`'s own stdout is a pipe when the test spawns
it, and a pipe's kernel buffer is smaller than the 100,000-byte body
the large-body test sends. A first version of that test read the
child's stdout only after the HTTP exchange finished — `wait_with_output`
after `post` returned — which deadlocks the moment the pipe fills:
`collect` blocks writing the rest of the body to a pipe nobody is
draining, so it never gets to send the response the test is waiting
on. Draining the pipe on its own thread, concurrently with the
exchange, is the fix — the same shape `fetch_speaks_http_1_0_and_streams_the_body`
already uses for the send side. Not a bug in a rule that traps; a
liveness bug ordinary tools do not catch until the deadline expires,
found by writing a test whose body was actually large enough to fill
one buffer.

---

## 6. What slice 3 builds

Three builtins replace the `extern fn socket`/`setsockopt`/`bind`/
`listen`/`accept` five `examples/serve/` and `examples/collect/` still
declare by hand against `Ffi("libc")`:

- **`bind(net, port) -> [net_in(bound)] int`** folds `socket`,
  `setsockopt(SO_REUSEADDR)` and `bind` into one call, the inbound
  mirror of `connect`'s `socket`+`connect` (`docs/connect.md` §9). It
  builds the same `struct sockaddr_in` `serve.ls`'s own `serve`
  function builds by hand: family bytes, the port big-endian, and
  `INADDR_ANY` — eight zero bytes where `connect`'s has four octets,
  because a listener binds every address the host has, not one it
  chose.
- **`listen(fd, backlog) -> [] int`** and **`accept(fd) -> [] int`**
  are thin: `listen(2)` and `accept(2)` with the peer address ignored
  (`NULL, NULL`, the same as `serve.ls`'s own `accept(libc, fd, 0, 0)`).
  Neither takes a capability — the fd is what `bind` already proved
  the authority for — so both are fixed signatures, checked the way
  `Builtin::Close` is, not the way `connect` and `bind` are.

### 6.1 `Net.in(port)`: the bound is a port, not `host:port`

`net.md` §2.1's table draws the line: outbound's bound is *which
host*, inbound's is *which port* — nothing else, because who may reach
a bound port is the perimeter's decision, never the program's. So a
`Net` narrowed for `bind` carries just the port as text —
`narrow(net, "8080")` — and `bind`'s row is `net_in(bound)`, checked
against the capability the same way `connect`'s port half already is
(§10.1): equality, not a prefix, because a bound of `"8080"` names one
port. Reusing `Net` rather than adding a second capability type keeps
the two halves of `docs/net.md` §2.1's table one type with two
meanings for its one string field, exactly as the table already
implied.

Only `bind` performs `net_in`. `listen` and `accept` perform nothing,
the same reasoning `close`'s empty row has: ending or operating a
resource whose authority was already spent when it was acquired is
not itself a new use of anything. (`file_read`'s row is the
counter-example and is not one here: it spends bytes off a path-bound
budget on every call, where `accept` spends nothing a bound could ever
name — the perimeter, not the row, is what still decides who gets
through.)

### 6.2 A correction, found while writing the same check twice

Writing `bind`'s port check by the same shape as `connect`'s
(`docs/connect.md` §10.1) surfaced a bug already latent in `connect`'s:
a bound whose port half does not parse as a number (`narrow(net,
"host:abc")`, which `narrow` never refuses — a bound is free-form text)
made `connect`'s `port_bound` `None`, the same value an *absent* port
uses for "no restriction". A malformed bound is not the same fact as
an unnarrowed one, and reading them alike would restrict a port that
was meant to be bound to nothing at all. Both parses now read a
non-empty, non-numeric half as -1 — a port `connect`/`bind` can never
be asked to dial, so a malformed narrowing refuses every call rather
than none. This does not widen what a program can reach: `narrow` only
ever narrows from what a capability already held, so failing to
*tighten* as intended was never a way to gain authority — but it was a
row that would have promised a bound it did not enforce, which is the
one thing `docs/reach.md` says a row must never do.

### 6.3 A symbol collision, found writing the first program to hit it

Testing `bind`/`listen`/`accept` against a real connection needed
`read`, `write` and `close` on the accepted socket, which still cross
through a program's own `extern fn` declarations (§6 above; `close`
is not one of the three builtins this slice adds). Declaring `extern
fn close` in that test alongside `bind` failed to compile:
`IncompatibleSignature` on the linker symbol `close`, because it is
declared twice at two different widths. Every `extern fn` in this
compiler crosses a lex-sys `int` as 64 bits, whatever the C function's
own parameter or return width — `abi.rs`'s `leaves_into` always
answers `types::I64` for `Type::Int`, foreign calls included. `bind`'s
own internal `close` (the socket-failure path, `docs/net.md` §3's
"sockets have to come out of libc"), like `open_file`'s in
`filesystem.md` §2, is declared against libc's true 32-bit `int`
instead, because that is the signature libc's own `close(2)` actually
has. One name, two declared widths, and Cranelift is right to refuse
a module that asks for both.

This did not need fixing to finish this slice — `close` is one call
in a test with nothing left to do once it returns, so the test simply
does not declare or call it, and the OS reclaims both descriptors when
the process exits. It is recorded here because it is not particular to
`bind`: any program combining an internal socket or file builtin with
an `extern fn` of the same libc name hits it, and `open_read`/
`fs_read`/`fs_write` have shared this exposure with `file_op`'s own
internal `open`/`close` since before `Net` existed. Whether the right
fix is widening the internal declarations to match `extern fn`'s
convention, or something else, is a question for a document of its
own — not answered by avoiding it once.

---

## 7. The suite

| Test | What it pins | § |
|---|---|---|
| `an_inbound_agent_reads_several_requests_in_a_row` | `collect` accepts three connections in a row without restarting, and reads each body whole | 2 |
| `collect_reads_a_body_larger_than_one_read` | a body that arrives across more than one `read`, larger than the 4 KiB a request line alone would need, drained concurrently with the exchange | 2, 5 |
| `the_network_programs_are_counted` | inbound 2, outbound 2 | 4 |
| `a_lex_sys_listener_accepts_a_real_connection` | `bind`/`listen`/`accept`, built, answer a real client over loopback | 6 |
| `binding_the_wrong_port_traps` | `bind`'s port is checked against the capability's bound, the inbound mirror of `connecting_to_the_wrong_port_traps` | 6.1 |
