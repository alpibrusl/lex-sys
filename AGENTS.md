# Writing cancho

> The entry point this repository did not have. `docs/` is **90,000
> words across 42 documents** and none of them is a first page, so an
> agent asked to write cancho had the choice of reading all of it or
> guessing. This is the short version, and every rule in it is here
> because something in this repository got it wrong first.
>
> `cancho agent-guidelines` prints this file. The compiler carries it,
> so a checkout is not required to read it.
>
> **Every checked code block below is run by the test suite.** The ones
> marked `cancho` must compile; the ones marked `cancho-refused` must
> be refused, with the rule they name. A guideline that stops being true
> is a red build, not a stale paragraph.

---

## 0. The loop

```sh
cancho check src/*.cho --std               # does it type-check?
cancho check src/*.cho --std --output json # …and which rule, as data
cancho authority src/*.cho --std           # what can it reach?
cancho run src/*.cho --std                 # build and run in one step
cancho fmt --check src/ tests/            # is it canonical? (`fmt src/` rewrites; docs/formatting.md)
cancho test tests/*.cho --std              # run every `fn test_*`; exit 4 if one failed (docs/testing.md)
```

`check` reports **every** independent refusal, not the first. On a
failure, read the `rule` field rather than the sentence: it is a stable
name, there are 58 of them, and `docs/agent-errors.md` is the contract.
One of them, `internal`, is the compiler's own failure, not your
program's (`docs/internal-errors.md`).

`cancho skill` and `cancho introspect` are how *this* page and the
full command surface stay the same age: both are generated from one
registration (`src/acli.rs`), not copied by hand, so a new command
cannot ship without them (`docs/agent-cli.md`). Their "Exit codes" /
"Output format" sections are the ACLI SDK's own generic template,
though, not this binary's -- the real exit codes are at the top of
`main.rs` and in `docs/agent-errors.md`.

**Do not regenerate a body because it was refused.** Every rule below
names what to change.

---

## 1. Ownership moves, and there is no `&mut self`

`std.buffer`, `std.vec` and `std.list` take their resource **by value**
and hand it back. A loop that fills one moves it round and round:

```cancho
import std.buffer;

fn collect[&h, &i](heap: &!h Heap, io: &!i Io) -> [heap, io_read] buffer.Buffer {
    var out = buffer.empty(heap, 64);
    var c = getchar(io);
    while c >= 0 {
        // The shape that repeats everywhere: take it, hand it back.
        out = buffer.push(heap, out, byte_of(c));
        c = getchar(io);
    }
    return out;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(args);
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            let text = collect(h, i);
            buffer.drop(h, text);
        }
    }
    release(heap); release(io);
    return 0;
}
```

`out = f(h, out, x)` is three tokens longer than `f(&mut out, x)` every
single time, and it is the cost of linearity stated honestly
(`docs/porting.md` §9.2 — six sites in one program).

**A function that owns a resource and can fail must hand it back on the
failing path too.** `read_file` answers `(Buffer, int)` for exactly that
reason: a tuple is not elegant, and it is not something you can forget
to write.

---

## 2. A `res` value is consumed exactly once, on every path

This is the largest family of refusals in the suite after plain type
errors — 19 fixtures. Four ways to get it wrong:

```cancho-refused
//~ RULE linear-value-unconsumed

// Nothing consumes `heap` before the block ends.
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(args);
    return 0;
}
```

The others: using one after it has moved (`linear-use-after-move`, 8
fixtures), consuming it in one branch and not another, and taking a
field out of it instead of taking the whole value apart.

**A capability is a `res` value like any other.** `main` owns what
`split` hands it and must `release` each one; a helper **borrows**.

---

## 3. Own or borrow, and the row follows

> Owning discharges. Borrowing declares.

`main` owns its capabilities, so its row is `[]` — that is not a gap,
it is the parameter list saying something stronger. A function handed
`&!i Io` says `[io_write]`, because that is what it did with it.

```cancho
import std.io;

// Borrowed, so the row is exact.
fn greet[&i](io: &!i Io) -> [io_write] int {
    return io.write_all(io, "hello\n");
}

// Owns, so the row is empty.
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(heap); release(args);
    borrow mut io as &!i in { greet(i); }
    release(io);
    return 0;
}
```

### 3.1 Narrow the body, never widen the row

An effect row is **exact in both directions**: a label the body performs
must appear, and a label that appears must be performed.

```cancho-refused
//~ RULE effect-not-declared

import std.io;

fn greet[&i](io: &!i Io) -> [] int {
    return io.write_all(io, "hello\n");
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(heap); release(args);
    borrow mut io as &!i in { greet(i); }
    release(io);
    return 0;
}
```

The fix is `[io_write]` here, because that is what the body does. Where
a row is wider than the body, **narrow the body** — do not widen the
row to make the checker stop. `cancho authority` prints the union of
what a program reaches, and a row that over-declares makes that report
a lie.

Labels: `io_read`, `io_write`, `err_write`, `fs_read(p)`, `fs_write(p)`,
`heap`, `args`, `ffi(lib)`.

### 3.2 Knowing you were asked to stop is a capability, not `Ffi("libc")`

`edition 6;` adds `Signals`, the eighth field of `Split`. Narrow it to the
signals you claim, and the row says which: `signals("INT,TERM")`, which
`cancho authority` prints and which keeps the report bounded (the
`sigblock`/`signal` workaround through `Ffi("libc")` made it `UNBOUNDED`).
`signals_pending` answers a bitmask of what arrived since the last call and
never waits; `poller_add_signals` makes the claim something a `Poller`
wakes for; `signals_close` ends it, and after it the next signal ends the
process, which is "a second signal kills at once". Claim before the first
`spawn` (`docs/signals.md`).

```cancho
edition 6;
import std.signals as sg;

fn stopped[&w](watch: &!w SignalWatch) -> [signals_read] bool {
    return sg.any(signals_pending(watch), sg.stop_signals());
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args, net, clock, signals } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    release(net); release(clock);
    let claim = narrow(signals, "INT,TERM");
    var status = 1;
    borrow claim as &s in {
        match signals_watch(s) {
            Watching::Ok(w) => {
                var watch = w;
                borrow mut watch as &!wh in {
                    if !stopped(wh) { status = 0; }
                }
                signals_close(watch);
            }
            Watching::Failed(e) => { status = 2; }
        }
    }
    release(claim);
    return status;
}
```

### 3.3 Foreign code: say which library, and read which symbols

`Ffi` is the one capability whose label does not bound what it authorises, so a program that calls C reports `bounded: false`. What it reports *with* that is exact:
`cancho authority` lists every foreign symbol the program can reach as `scope:symbol` (`unbounded_by`, one per line in `--output json`, so a CI pin diffs an added symbol as one added
line). A foreign function borrows **exactly one** `Ffi`, naming **one** library (anything else is `foreign-declaration`: a declaration with no capability used to be accepted and
reported `bounded: true`). A program that calls two libraries narrows to a **set**, written in any order and answered alphabetically, and lends each function only what it needs:

```cancho
edition 5;

extern fn labs[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int;
extern fn pthread_self[&f](ffi: &f Ffi("libpthread")) -> [ffi("libpthread")] int;

fn magnitude[&f](ffi: &f Ffi("libc"), n: int) -> [ffi("libc")] int {
    return labs(ffi, n);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args, net, clock } = split(world);
    release(io); release(fs); release(heap); release(args); release(net); release(clock);
    let native = narrow(ffi, "libpthread,libc");
    var status = 1;
    borrow native as &f in {
        if magnitude(f, 0 - 7) == 7 && pthread_self(f) != 0 { status = 0; }
    }
    release(native);
    return status;
}
```

The library in `Ffi("...")` is a **claim** the declaration makes; the symbol is the fact. A set is not a text prefix (`Ffi("libc")` no longer narrows to `Ffi("libcrypto")`), and a
malformed one is `foreign-scope` (`docs/foreign-authority.md`).

### 3.4 Two paths: narrow into several

A program that reads two unrelated files (entropy from `/dev/urandom` and a certificate directory) cannot hold one `Fs` for both without holding `Fs("")`, and then its report says
`fs_read("")`, which is every file. `narrow` takes one literal or several. With several it consumes the `Fs` (or `Exec`) and answers a **tuple**, one capability narrowed to each literal,
in the order written; each helper then borrows the one it needs and declares one label:

```cancho
fn entropy[&f, &o](fs: &f Fs("/dev/urandom"), out: &!o [byte]) -> [fs_read("/dev/urandom")] int {
    return fs_read(fs, "/dev/urandom", out);
}

fn trust[&f, &o](fs: &f Fs("/etc/ssl/certs/ca-certificates.crt"), out: &!o [byte]) -> [fs_read("/etc/ssl/certs/ca-certificates.crt")] int {
    return fs_read(fs, "/etc/ssl/certs/ca-certificates.crt", out);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(heap); release(args);
    let (rng, ca) = narrow(fs, "/dev/urandom", "/etc/ssl/certs/ca-certificates.crt");
    var got = 0;
    region r {
        let seed = alloc_slice[r](32, byte_of(0));
        let pem = alloc_slice[r](4096, byte_of(0));
        borrow rng as &x in { got = got + entropy(x, seed); }
        borrow ca as &y in { got = got + trust(y, pem); }
    }
    release(rng);
    release(ca);
    return 0;
}
```

The report names those two paths and nothing under the filesystem else. The literals must be **unrelated**: two equal, or one inside another, is `capability-not-narrowable` (the inner
one grants nothing the outer does not), as is a literal that does not extend the capability's own path. A path chosen at run time cannot be a literal; the narrowest form for it is a
fixed *directory* and a run-time name beneath it (`examples/tls_echo_fixed`). `Ffi` and `Signals` narrow to a *set* in one capability (`narrow(ffi, "libc,libm")`), and more than one
literal on them, or on `Net`, is refused. [`docs/narrowing-into-several.md`](docs/narrowing-into-several.md) has the rule and its limits.

### 3.5 A serial port is a capability, not a termios escape hatch

Edition 8. A serial device is reached through `Tty`, which narrows by a
device-path prefix exactly as `Fs` does — the same prefix rule, the same
`narrow` forms, and the same refusal for an empty literal. The port
handle is `Port`, a `res` value with one descriptor leaf (`Udp`'s shape),
and the verbs are `tty_open`, `tty_configure` (raw mode, 8N1, a speed —
one call, no mode algebra), `tty_read` (never blocks; the poller is the
waiting story), `tty_write`, `tty_flush_input` and `tty_close`, plus
`poller_add_tty`:

```cancho
edition 8;

fn ping[&t, &b](tty: &t Tty("/dev"), bus: &b [byte]) -> [tty_open("/dev"), tty_read, tty_write] int {
    match tty_open(tty, "/dev/ttyACM0") {
        TtyOpened::Ok(port) => {
            var got = 0 - 1;
            borrow mut port as &!p in {
                if tty_configure(p, 1000000) == 0 {
                    region r {
                        let reply = alloc_slice[r](16, byte_of(0));
                        tty_write(p, bus);
                        got = tty_read(p, reply);
                    }
                }
            }
            tty_close(port);
            return got;
        }
        TtyOpened::Failed(errno) => { return 0 - 1; }
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args, net, clock, signals, exec, tty } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    release(net); release(clock); release(signals); release(exec);
    region r {
        let bus = alloc_slice[r](1, byte_of(0));
        let device = narrow(tty, "/dev");
        borrow device as &t in { ping(t, bus); }
        release(device);
    }
    return 0;
}
```

The termios layout, the octal B-constants and the macOS `IOSSIOSPEED`
two-step are the backend's, written once in Rust — not nine `extern fn`
declarations carrying a per-platform ABI in program source, which is
what the robot's spike measured and the whole reason the capability
exists ([`docs/tty.md`](docs/tty.md)). The report answers `tty_open("/dev")`
with the prefix and **no `foreign` line**: a program that moves a robot
can be `bounded: true`.


---

## 4. A reference may not outlive its region

11 fixtures. A region is an arena, a `borrow` block, or a caller's
region parameter, and nothing that points into one may leave it:

```cancho-refused
//~ RULE reference-escapes-region

fn leak() -> [] &static [byte] {
    region a {
        let bytes = alloc_slice[a](4, byte_of(65));
        return bytes;
    }
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    return 0;
}
```

Take the region as a parameter instead — `fn f[&r](…) -> [] &r [byte]`
— so the caller decides how long it lives.

---

## 5. `res` and `val`, and what a container may hold

An arena and a boxed slice hold `val` data **only**: the fill is copied
into every element, and freeing the run is one `free` that runs nothing,
so a linear obligation put inside would be dropped rather than
discharged. 11 fixtures.

`std.vec` is `Vec[T: val]`, honestly. **`std.list` is the collection
that holds resources**, and the difference is the *shape* rather than
the generics — `docs/collections.md`.

---

## 6. Five things that cost this repository a compile each

Not rules so much as facts. Each was found by writing a program, and
each is a checked block below the table — **because the table itself was
wrong once.**

> **Corrected (#63).** This section shipped with a sixth row saying
> *"there is no unary minus; write `0 - x`."* There is one: `UnOp::Neg`
> is in the AST, `-x` parses, and it works on `int` and `float` alike —
> `examples/newton.cho` had been using it since before that row was
> written. The claim came from a golden-hash fixture that used `0 - a`,
> and reading a fixture is not reading the language.
>
> The checked code blocks in this document are run by the suite. This
> table was not, which is exactly why the wrong thing survived in it. It
> is now.

| | |
|---|---|
| **An arena is one 64 KiB chunk, and exhaustion traps** | Not an error — SIGILL. `examples/cut/` sized a line buffer to what fit beside its bitmap, and `line-reading.md` §2 is what that cost: a longer line was silently truncated and the program answered the wrong field. **Use `std.buffer` on the heap when the size is not known** |
| **`alloc_slice` already yields a reference** | A slice *is* a reference. `borrow mut` on one is a reference to a reference, and the refusal says `expected [byte], found &!a [byte]` |
| **A struct field cannot be `[T]`** | *"`[T]` has no size of its own."* Use a reference, or keep the slice beside the struct rather than in it |
| **Six escapes, and no `\x` or `\u`** | `\n \r \t \\ \" \0`. A source file is already UTF-8, so `"café 日 😀"` needs none — `docs/strings.md` §8 |
| **A character is written as one: `'a'` is 97** | A third spelling of an integer, not a type — so `'0' + n % 10`, and `byte_of('\n')` where a `byte` is wanted. Same six escapes with `\'` for `\"`, and non-ASCII is refused — `docs/character-literals.md` |
| **`len` is a builtin and may not be redeclared** | Nor may any other prelude name. `std.buffer` calls its length `size` for this reason |

Each of those, in a program the suite compiles:

```cancho
fn facts(x: float) -> [] float {
    // Unary minus exists, on `int` and `float` alike -- see the
    // correction above.
    let negated = -x;
    // `sqrt` is a builtin and needs no capability: it is one
    // instruction, so it reaches no library, and its row is `[]`
    // (`docs/float-math.md` §3).
    return negated + sqrt(4.0);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(args);

    // Six escapes and no `\x`; a source file is already UTF-8, so a
    // literal says what it means rather than spelling it out.
    let text = "café 日 😀";
    var size = len(text);

    // `alloc_slice` already yields a reference: no `borrow mut`.
    region a {
        let room = alloc_slice[a](16, byte_of(0));
        size = size + len(room);
        // A character literal is an `int`, so it adds to one directly and
        // needs `byte_of` to become a byte -- the same conversion a
        // number needed, with an argument that can be read.
        room[0] = byte_of('0' + size % 10);
    }
    release(heap);

    if facts(2.0) < 0.0 {
        return size;
    }
    return 0;
}
```

```cancho-refused
//~ RULE unsized-type

// A struct field cannot be `[T]`: it has no size of its own.
struct Holder { bytes: [byte] }

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(ffi); release(fs); release(heap); release(args);
    return 0;
}
```

---

## 7. Use the library

`std.bytes`, `std.io`, `std.math`, `std.buffer`, `std.option`,
`std.result`, `std.list`, `std.vec`, `std.fmt`, `std.fmt32` (`f32` text,
edition 6), `std.bignum`, `std.utf8`, `std.flags`, `std.crypto`, `std.ed25519`, `std.json`,
`std.map`, `std.http`, `std.route`, `std.test`. Pass `--std` and write the `import` — there is no prelude.

`std.json` parses into a **tape** you provide (`docs/json.md`) rather than
building a tree, and writes through a `Writer` you move from call to
call, like a `Buffer`:

```cancho
import std.buffer;
import std.json;

// Read `age` from a request body; answer `{"next_age":N}`, or `{"error":...}`.
fn reply[&h, &s](heap: &!h Heap, body: &s [byte]) -> [heap] buffer.Buffer {
    var w = json.writer(heap, 64);
    region a {
        let tape = alloc_slice[a](json.tape_len(body), 0);
        let nodes = json.parse(body, tape);
        w = json.begin_object(heap, w);
        if nodes < 0 {
            w = json.put_key(heap, w, "error");
            w = json.put_string(heap, w, json.error_message(json.error_code(nodes)));
        } else {
            w = json.put_key(heap, w, "next_age");
            w = json.put_int(heap, w, json.to_int(body, tape, json.get(body, tape, 0, "age")) + 1);
        }
        w = json.end_object(heap, w);
    }
    return json.finish(w);
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(args); release(io);
    borrow mut heap as &!h in {
        let ok = reply(h, "{\"age\": 41}");
        let bad = reply(h, "{\"age\": }");
        buffer.drop(h, ok);
        buffer.drop(h, bad);
    }
    release(heap);
    return 0;
}
```

A parse error is a value (`nodes < 0`), and a writer misused -- a key
outside an object, a value with no key -- is a trap, not bad JSON.

`std.http` parses one request head into an integer table you provide and
`std.route` maps a method and path to an id you chose (`docs/http.md`);
you `match` on the id, so there is no handler registry:

```cancho
import std.buffer;
import std.http;
import std.route;

// One request in, one response out: 200 with the `:id` segment, 404, 405 or 400.
fn handle[&h, &r, &q](heap: &!h Heap, router: &r route.Router, request: &q [byte]) -> [heap] buffer.Buffer {
    let table = box_slice(heap, http.slots(32), 0);
    let params = box_slice(heap, 2 * route.most_params(router), 0);
    var out = buffer.empty(heap, 256);
    borrow mut table as &!tw in {
        borrow mut params as &!pw in {
            let t = contents(tw);
            let p = contents(pw);
            let n = http.parse(request, t);
            if n < 0 {
                out = http.respond_head(heap, out, 400, "text/plain", 0, false);
            } else {
                let path = http.path(request, t);
                let id = route.find(router, http.method(request, t), path, p);
                if id == 1 {
                    let who = path[p[0]..p[1]];
                    out = http.respond_head(heap, out, 200, "text/plain", len(who), http.keeps_alive(t));
                    out = buffer.append(heap, out, who);
                } else if id == 0 - 2 {
                    out = http.respond_head(heap, out, 405, "text/plain", 0, false);
                } else {
                    out = http.respond_head(heap, out, 404, "text/plain", 0, false);
                }
            }
        }
    }
    unbox_slice(heap, params);
    unbox_slice(heap, table);
    return out;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi); release(fs); release(args); release(io);
    borrow mut heap as &!h in {
        var router = route.empty(h);
        router = route.add(h, router, "GET", "/users/:id", 1);
        borrow router as &r in {
            let ok = handle(h, r, "GET /users/42 HTTP/1.1\r\nHost: x\r\n\r\n");
            let wrong = handle(h, r, "DELETE /users/42 HTTP/1.1\r\nHost: x\r\n\r\n");
            buffer.drop(h, ok);
            buffer.drop(h, wrong);
        }
        route.drop(h, router);
    }
    release(heap);
    return 0;
}
```

An incomplete head is a value too (`http.is_incomplete`): read more and call
`parse` again. A request that is ambiguous about where its body ends --
obsolete line folding, two different `Content-Length`s, a `Transfer-Encoding`
beside a length -- is refused, not guessed at.

**A function earns its way into `std` by a program asking for it**, and
that is how most of these arrived — `std.crypto` is named as the one
exception in `docs/crypto.md`'s own header. If you need something that
is not there, write it in your program first; it moves into the library
when a
*second* program needs it.

---

## 8. What this language does not have

Saying these plainly saves a cycle: no borrow checker (regions instead),
no traits, no closures or function values, no `comptime` beyond
`static` and constant folding, no async, no generics over effects, no
warnings — a diagnostic is a refusal.

`int` is 64-bit and **traps** on overflow rather than wrapping;
`wrapping_add` and its two siblings are how you ask for wraparound when
that is the intent.

---

## 9. Where to read more

| | |
|---|---|
| The rules of the type system | `docs/linearity-and-effects.md` |
| What a refusal means, as data | `docs/agent-errors.md` |
| The CLI as data, not a doc that can drift | `docs/agent-cli.md` |
| What a program can reach, and how it is reported | `docs/reach.md`, `docs/authority.md` |
| Which collection holds a resource | `docs/collections.md` |
| Everything else | `docs/README.md` indexes all 42, with a one-line status each |
