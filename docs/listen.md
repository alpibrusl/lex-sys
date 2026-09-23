# A second inbound program

> **Status: a probe**, the inbound counterpart to
> [`connect.md`](connect.md). `net.md` §5 and `connect.md` §6 count
> inbound and outbound separately, each against a bar of two askers.
> `connect.md`'s two programs cleared outbound. This is the one that
> tries to clear inbound.

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

## 6. The suite

| Test | What it pins | § |
|---|---|---|
| `an_inbound_agent_reads_several_requests_in_a_row` | `collect` accepts three connections in a row without restarting, and reads each body whole | 2 |
| `collect_reads_a_body_larger_than_one_read` | a body that arrives across more than one `read`, larger than the 4 KiB a request line alone would need, drained concurrently with the exchange | 2, 5 |
| `the_network_programs_are_counted` | inbound 2, outbound 2 | 4 |
