#!/usr/bin/env python3
"""Measure what the LLVM backend costs (or saves) against Cranelift, on the
programs that currently build on both.

`docs/llvm-backend.md` §7 is what these numbers mean. Unlike `bench.py`,
which compares two *source programs* (checked vs. wrapping) through one
backend, this compares one *source program* through two backends --
`--backend cranelift` (the default) against `--backend llvm`
(`docs/llvm-backend.md` §4).

Nine of `benches/`'s programs build on both backends today: the rest of
the suite still needs `box_slice` (heap-boxed slices), bare `alloc[a]`,
`arg_count`, `Type::Float` or `getchar`/`io_read`, none of which the LLVM
backend lowers yet (§7.5's gap inventory; `wrapping_add`/`sub`/`mul`
closed in §7.4, `region`/`alloc_slice`/`byte_of`/`Expr::Not` in §7.5).
`sum_checked.ls`/`sum_wrapping.ls`, `fib_checked.ls`/`fib_wrapping.ls`
and `sieve_checked.ls`/`sieve_wrapping.ls`/`scan_checked.ls`/
`scan_wrapping.ls` communicate correctness through their exit code
(`result - expected`, zero when right); `mandelbrot.ls` also prints a
checksum, which is compared as well as timed. The checked/wrapping pairs
are also `docs/overflow-cost.md`'s own question -- `scripts/bench.py`
measures it under Cranelift; this is the first
measurement of it under `--backend llvm`.

Runs are **interleaved** (cranelift, llvm, cranelift, llvm, ...) so that
thermal drift and scheduler noise land on both halves, and the reported
figure is the minimum, which is the least noisy estimator for a
deterministic workload -- the same method `bench.py` already uses.

`--with-c` adds a third, three-way interleaved comparison against
`benches/three/mandelbrot.c` -- the same kernel `docs/against-c-and-rust.md`
already runs lex-sys/cranelift against C and Rust on, and the one §7's own
falsifier ("if an LLVM backend lands and the gap stays at 1.6x, the claim
was wrong") is about.

    python3 scripts/backend_compare.py
    python3 scripts/backend_compare.py --rounds 15
    python3 scripts/backend_compare.py --with-c
"""

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent

# (label, source path relative to ROOT, expected stdout or None)
PROGRAMS = [
    ("sum_checked", "benches/sum_checked.ls", None),
    ("sum_wrapping", "benches/sum_wrapping.ls", None),
    ("fib_checked", "benches/fib_checked.ls", None),
    ("fib_wrapping", "benches/fib_wrapping.ls", None),
    ("sieve_checked", "benches/sieve_checked.ls", None),
    ("sieve_wrapping", "benches/sieve_wrapping.ls", None),
    ("scan_checked", "benches/scan_checked.ls", None),
    ("scan_wrapping", "benches/scan_wrapping.ls", None),
    ("mandelbrot", "benches/three/mandelbrot.ls", "39690297"),
]


def compiler() -> pathlib.Path:
    for profile in ("release", "debug"):
        exe = ROOT / "target" / profile / "lex-sys"
        if exe.exists():
            return exe
    sys.exit("build the compiler first: cargo build --release")


def build(source: pathlib.Path, target: pathlib.Path, backend: str) -> None:
    run = subprocess.run(
        [str(compiler()), "build", str(source), "--std", "--backend", backend, "-o", str(target)],
        capture_output=True,
        text=True,
    )
    if run.returncode != 0:
        sys.exit(f"{source.name} did not compile with --backend {backend}:\n{run.stderr}")


def time_once(exe: pathlib.Path, expected_stdout):
    start = time.perf_counter()
    run = subprocess.run([str(exe)], capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if run.returncode != 0:
        sys.exit(f"{exe.name} exited {run.returncode}: it computed the wrong answer")
    if expected_stdout is not None and run.stdout.strip() != expected_stdout:
        sys.exit(f"{exe.name} printed {run.stdout.strip()!r}, expected {expected_stdout!r}")
    return elapsed


def compare(left: pathlib.Path, right: pathlib.Path, rounds: int, expected_stdout):
    """Interleaved A/B. Returns (samples_left, samples_right)."""
    a, b = [], []
    for _ in range(rounds):
        a.append(time_once(left, expected_stdout))
        b.append(time_once(right, expected_stdout))
    return a, b


def compare_three(exes: dict, rounds: int, expected_stdout):
    """Interleaved across every key of `exes`, in the same fixed order each
    round -- the same idea `compare` uses, extended past a pair."""
    samples = {name: [] for name in exes}
    for _ in range(rounds):
        for name, exe in exes.items():
            samples[name].append(time_once(exe, expected_stdout))
    return samples


def spread(samples) -> float:
    lo, hi = min(samples), max(samples)
    return 0.0 if lo == 0 else (hi - lo) / lo * 100


def row(name: str, cranelift, llvm) -> str:
    lo_c, lo_l = min(cranelift), min(llvm)
    percent = (lo_l / lo_c - 1) * 100
    return (
        f"{name:<14} {lo_c:>9.4f}s ({spread(cranelift):>4.1f}%) "
        f"{lo_l:>9.4f}s ({spread(llvm):>4.1f}%) {percent:>+8.1f}%"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--rounds", type=int, default=9, help="runs per backend")
    parser.add_argument(
        "--with-c", action="store_true", help="also run benches/three/mandelbrot.c, three-way interleaved"
    )
    parser.add_argument("--cc", help="which C compiler to use with --with-c")
    args = parser.parse_args()

    scratch = pathlib.Path(tempfile.mkdtemp(prefix="lex-sys-backend-compare-"))
    try:
        print(
            f"{'program':<14} {'cranelift':>9} {'(spread)':>7} "
            f"{'llvm':>9} {'(spread)':>7} {'llvm/cranelift':>14}"
        )
        print("-" * 66)
        mandelbrot_exes = None
        for name, relative, expected_stdout in PROGRAMS:
            source = ROOT / relative
            exes = {}
            for backend in ("cranelift", "llvm"):
                target = scratch / f"{name}_{backend}"
                build(source, target, backend)
                exes[backend] = target
            cranelift, llvm = compare(exes["cranelift"], exes["llvm"], args.rounds, expected_stdout)
            print(row(name, cranelift, llvm))
            if name == "mandelbrot":
                mandelbrot_exes = exes

        print()
        print(f"{len(PROGRAMS)} of `benches/`'s programs build on both backends today;")
        print("the rest need `box_slice`, `alloc`, `arg_count`, `Type::Float` or `getchar` (docs/llvm-backend.md §7.5).")

        if args.with_c:
            wanted = (args.cc,) if args.cc else ("clang", "gcc")
            cc = next((c for c in wanted if shutil.which(c)), None)
            if cc is None:
                print("\nno C compiler on the path; skipping --with-c")
                return
            c_exe = scratch / "mandelbrot_c"
            build_c = subprocess.run(
                [cc, "-O2", "-DCHECKED=1", str(ROOT / "benches/three/mandelbrot.c"), "-o", str(c_exe)],
                capture_output=True,
                text=True,
            )
            if build_c.returncode != 0:
                sys.exit(f"mandelbrot.c did not compile with {cc}:\n{build_c.stderr}")
            three = {"cranelift": mandelbrot_exes["cranelift"], "llvm": mandelbrot_exes["llvm"], cc: c_exe}
            samples = compare_three(three, args.rounds, "39690297")
            mins = {name: min(times) for name, times in samples.items()}
            print(f"\nmandelbrot, three-way interleaved against {cc} -O2 ({args.rounds} rounds):")
            for name, lo in mins.items():
                ratio = lo / mins[cc]
                print(f"  {name:<10} {lo:.4f}s  ({ratio:.3f}x {cc})")
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    main()
