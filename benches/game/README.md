# `benches/game/`

Programs from the [Computer Language Benchmarks
Game](https://benchmarksgame-team.pages.debian.net/benchmarksgame/),
ported to lex-sys and to C. `docs/benchmarks-game.md` is what the
numbers mean.

## The rules

From **are-we-fast-yet** (Marr et al., DLS 2016) rather than from the
Game's leaderboard:

- **The same algorithm**, line for line, in both languages.
- **The same output**, checked against the value the Game publishes —
  on every run of `scripts/game.py` and in the conformance suite.

The C here is deliberately **not** the Game's own entry. Those are
hand-vectorised and threaded; comparing against them measures a decade
of tuning by motivated experts, which is a different question and one
the Game's own maintainers caution against.

## What is here

| | what it stresses | measured |
|---|---|---|
| `fannkuch.ls` / `.c` | Integer arrays, branches | 1.32× |
| `spectral.ls` / `.c` | Float compute, a division in the inner loop | 2.58× |
| `binarytrees.ls` / `.c` | `malloc` and `free` | 1.17× |

Each takes `N` on the command line and defaults to the size its header
states an expected output for.

## Running them

```sh
cargo build --release
python3 scripts/game.py            # full sizes, with the spread
python3 scripts/game.py --quick    # smaller, for a sanity check
```

The harness checks every program's output against the published answer
before it reports a time. A fast wrong answer is not a result.
