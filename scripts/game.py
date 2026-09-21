#!/usr/bin/env python3
"""lex-sys against C on programs from the Computer Language Benchmarks Game.

`scripts/three.py` measures two kernels against C and Rust.  This measures
a wider set, for the reason `docs/benchmarks-game.md` §1 gives: two
kernels is not enough evidence for a single ratio, and it turned out not
to be one.

Two rules, both from are-we-fast-yet's methodology rather than from the
Benchmarks Game's leaderboard:

  * **The same algorithm**, line for line.  Not the Game's own entries,
    which are hand-vectorised and threaded -- comparing against those
    measures a decade of tuning by motivated experts.
  * **The same output**, checked every run against the value the
    Benchmarks Game publishes.  A mismatch fails the run rather than
    being reported as a time.

And one rule this file adds, because `three.py` did not have it:

  * **Report the spread.**  Best-of-N hides the distribution, so every
    row here carries the median and how far the samples ranged.  A ratio
    without one is a number nobody can check.

    python3 scripts/game.py
    python3 scripts/game.py --rounds 9
    python3 scripts/game.py --quick
"""

import argparse
import pathlib
import shutil
import statistics
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
GAME = ROOT / "benches" / "game"

# (name, timed N, verified N, the output the Benchmarks Game publishes for it)
PROGRAMS = [
    ("fannkuch", 11, 7, "228\nPfannkuchen(7) = 16\n"),
    ("spectral", 2000, 100, "1.274219991\n"),
    (
        "binarytrees",
        18,
        10,
        "stretch tree of depth 11\t check: 4095\n"
        "1024\t trees of depth 4\t check: 31744\n"
        "256\t trees of depth 6\t check: 32512\n"
        "64\t trees of depth 8\t check: 32704\n"
        "16\t trees of depth 10\t check: 32752\n"
        "long lived tree of depth 10\t check: 2047\n",
    ),
]

QUICK = {"fannkuch": 9, "spectral": 500, "binarytrees": 14}


def build(tmp: pathlib.Path, name: str) -> tuple[pathlib.Path, pathlib.Path]:
    """The lex-sys build and the C build of one program."""
    compiler = ROOT / "target" / "release" / "lex-sys"
    if not compiler.exists():
        sys.exit(f"build it first: cargo build --release   ({compiler} is missing)")
    ls = tmp / name
    subprocess.run(
        [str(compiler), "build", "--std", str(GAME / f"{name}.ls"), "-o", str(ls)],
        check=True,
    )
    c = tmp / f"{name}_c"
    subprocess.run(["cc", "-O2", str(GAME / f"{name}.c"), "-o", str(c)], check=True)
    return ls, c


def samples(exe: pathlib.Path, n: int, rounds: int) -> tuple[list[float], str]:
    times, output = [], None
    for _ in range(rounds):
        start = time.perf_counter()
        done = subprocess.run([str(exe), str(n)], capture_output=True, text=True)
        times.append(time.perf_counter() - start)
        if done.returncode != 0:
            sys.exit(f"{exe.name} exited {done.returncode}")
        output = done.stdout
    return times, output


def spread(times: list[float]) -> str:
    """How far the samples ranged, as a percentage of the fastest."""
    return f"{(max(times) - min(times)) / min(times) * 100:4.1f}%"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rounds", type=int, default=5, help="runs per build")
    parser.add_argument("--quick", action="store_true", help="smaller sizes")
    args = parser.parse_args()

    tmp = pathlib.Path(subprocess.run(["mktemp", "-d"], capture_output=True, text=True).stdout.strip())
    print(f"{'program':<14}{'N':>6}  {'lex-sys':>18}  {'C -O2':>18}   ratio")
    print(f"{'':14}{'':>6}  {'median  (spread)':>18}  {'median  (spread)':>18}")
    ratios = []
    try:
        for name, timed, verified, expected in PROGRAMS:
            ls, c = build(tmp, name)

            # Correctness first, at the size the Benchmarks Game publishes
            # an answer for. A fast wrong answer is not a result.
            for exe in (ls, c):
                _, got = samples(exe, verified, 1)
                if got != expected:
                    sys.exit(f"{exe.name} at N={verified} printed:\n{got!r}\nwanted:\n{expected!r}")

            n = QUICK[name] if args.quick else timed
            ls_times, ls_out = samples(ls, n, args.rounds)
            c_times, c_out = samples(c, n, args.rounds)
            if ls_out != c_out:
                sys.exit(f"{name}: the two builds disagree at N={n}")

            a, b = statistics.median(ls_times), statistics.median(c_times)
            ratios.append(a / b)
            print(
                f"{name:<14}{n:>6}  {a * 1000:>10.1f}ms ({spread(ls_times)})"
                f"  {b * 1000:>10.1f}ms ({spread(c_times)})   {a / b:.2f}x"
            )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    print()
    print(f"range {min(ratios):.2f}x to {max(ratios):.2f}x over {len(ratios)} programs")
    print("`docs/benchmarks-game.md` is what the numbers mean.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
