#!/usr/bin/env python3
"""Measure what the overflow trap costs, so the number is re-measured
rather than quoted.

Each benchmark in `benches/` is a pair: `<name>_checked.ls` uses `+`, `-`
and `*`, which trap on overflow; `<name>_wrapping.ls` is the same program
with `wrapping_add`/`wrapping_sub`/`wrapping_mul`, which lower to a bare
`iadd`/`isub`/`imul`. The difference between the two binaries is the cost
of the guarantee and nothing else.

Runs are **interleaved** (checked, wrapping, checked, wrapping, ...) so
that thermal drift and scheduler noise land on both halves, and the
reported figure is the minimum, which is the least noisy estimator for a
deterministic workload.

    python3 scripts/bench.py              # the lex-sys pairs
    python3 scripts/bench.py --with-c     # and the C comparison, if a
                                          # compiler is on the path
    python3 scripts/bench.py --with-c --cc gcc

`docs/overflow-cost.md` is what these numbers mean.
"""

import argparse
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCHES = ["sum", "sieve", "scan", "fib"]
# Each benchmark returns `result - expected`, so a non-zero exit is a wrong
# answer and the run is not reported as a time.
EXPECTED_EXIT = 0


def compiler() -> pathlib.Path:
    for profile in ("release", "debug"):
        exe = ROOT / "target" / profile / "lex-sys"
        if exe.exists():
            return exe
    sys.exit("build the compiler first: cargo build --release")


def build(source: pathlib.Path, target: pathlib.Path) -> None:
    run = subprocess.run(
        [str(compiler()), "build", str(source), "-o", str(target)],
        capture_output=True,
        text=True,
    )
    if run.returncode != 0:
        sys.exit(f"{source.name} did not compile:\n{run.stderr}")


def time_once(exe: pathlib.Path) -> float:
    start = time.perf_counter()
    run = subprocess.run([str(exe)], capture_output=True)
    elapsed = time.perf_counter() - start
    if run.returncode != EXPECTED_EXIT:
        sys.exit(f"{exe.name} exited {run.returncode}: it computed the wrong answer")
    return elapsed


def compare(left: pathlib.Path, right: pathlib.Path, rounds: int):
    """Interleaved A/B. Returns (min_left, min_right, percent)."""
    a, b = [], []
    for _ in range(rounds):
        a.append(time_once(left))
        b.append(time_once(right))
    lo_a, lo_b = min(a), min(b)
    return lo_a, lo_b, (lo_a / lo_b - 1) * 100


def row(name: str, checked: float, wrapping: float, percent: float) -> str:
    return f"{name:<10} {checked:>8.4f}s {wrapping:>10.4f}s {percent:>+9.1f}%"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rounds", type=int, default=9, help="runs per half")
    parser.add_argument("--with-c", action="store_true", help="also run benches/reduce.c")
    parser.add_argument("--cc", help="which C compiler to use with --with-c")
    args = parser.parse_args()

    scratch = pathlib.Path(tempfile.mkdtemp(prefix="lex-sys-bench-"))
    try:
        print(f"{'benchmark':<10} {'checked':>9} {'wrapping':>11} {'difference':>10}")
        print("-" * 43)
        for name in BENCHES:
            pair = []
            for half in ("checked", "wrapping"):
                source = ROOT / "benches" / f"{name}_{half}.ls"
                target = scratch / f"{name}_{half}"
                build(source, target)
                pair.append(target)
            print(row(name, *compare(pair[0], pair[1], args.rounds)))

        if args.with_c:
            wanted = (args.cc,) if args.cc else ("clang", "gcc")
            cc = next((c for c in wanted if shutil.which(c)), None)
            if cc is None:
                print("\nno C compiler on the path; skipping the comparison")
                return
            print()
            source = ROOT / "benches" / "reduce.c"
            pair = []
            for checked in (1, 0):
                target = scratch / f"reduce_{checked}"
                run = subprocess.run(
                    [cc, "-O2", f"-DCHECKED={checked}", str(source), "-o", str(target)],
                    capture_output=True,
                    text=True,
                )
                if run.returncode != 0:
                    sys.exit(f"reduce.c did not compile with {cc}:\n{run.stderr}")
                pair.append(target)
            # reduce.c prints its sum rather than returning it, so it exits 0
            # either way; the printed value is what says the halves agree.
            print(row(f"reduce/{cc}", *compare(pair[0], pair[1], args.rounds)))
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    main()
