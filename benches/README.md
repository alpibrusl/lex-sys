# `benches/`

What the overflow trap costs, measured rather than asserted.
[`docs/overflow-cost.md`](../docs/overflow-cost.md) is what the numbers
mean; this is how to get them.

```sh
cargo build --release
python3 scripts/bench.py              # the four pairs
python3 scripts/bench.py --with-c     # and the C comparison
```

## The shape

Each benchmark is a **pair**. `<name>_checked.ls` uses `+`, `-` and `*`,
which trap on overflow. `<name>_wrapping.ls` is the same program with
`wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
`iadd`/`isub`/`imul` with no trap. Nothing else differs, so the gap
between the two binaries is the guarantee and nothing else.

The wrapping halves are deliberately **not** idiomatic lex-sys.
`wrapping_add` means *the bits are the intent*
([`defined-behaviour.md`](../docs/defined-behaviour.md) §2.2); here the
intent is only to delete the check, which is the one thing it is not for.
They exist to be measured against, not to be copied.

Every program returns `result - expected`, so it exits `0` exactly when it
is right. That is what `every_benchmark_pair_agrees` checks in the
conformance suite, and it is why the timing is meaningful: the two halves
are known to be the same program.

## What each one is for

| Pair | What dominates | Why it is here |
|---|---|---|
| `sum` | arithmetic, nothing else | The ceiling. Nobody writes this loop; it bounds the answer from above |
| `sieve` | strided writes and cache | The check when it is not on the critical path |
| `scan` | comparisons and branches | Text-handling systems code — and the one that came out backwards |
| `fib` | calls and returns | Arithmetic looks different when the frame pointer is the bottleneck |
| `reduce.c` | — | The same guarantee given to a mature backend, which is how we know the cost is the semantics rather than Cranelift |

## Reading a number

The script interleaves the two halves and reports the minimum of nine
runs, because a minimum is the least noisy estimator for a deterministic
workload and interleaving puts thermal drift on both halves.

It is still one machine. A difference of a few percent is below what this
harness can resolve — `scan` moved between −5.8% and −10.5% purely from
shifting the code's address — so treat small numbers as "not the
bottleneck" rather than as measurements.
