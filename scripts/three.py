#!/usr/bin/env python3
"""lex-sys against C and Rust, on the same algorithm.

`scripts/bench.py` measures one language against itself — what the
overflow trap costs. This measures the language against the two it is
compared to, which is a different question and needs a different
discipline:

  * **The same algorithm**, line for line, not the same *task*. Comparing
    `examples/sort` to GNU `sort` would measure decades of tuning.
  * **The same semantics.** Rust's release profile wraps on overflow and
    lex-sys traps, so Rust is built both ways and the honest row is the
    one that also traps.
  * **The same answer.** Every build prints a checksum, and a mismatch
    fails the run rather than being reported as a time.

    python3 scripts/three.py
    python3 scripts/three.py --rounds 15

`docs/against-c-and-rust.md` is what the numbers mean.
"""

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
THREE = ROOT / "benches" / "three"


def compiler() -> pathlib.Path:
    for profile in ("release", "debug"):
        exe = ROOT / "target" / profile / "lex-sys"
        if exe.exists():
            return exe
    sys.exit("build the compiler first: cargo build --release")


def shell(command: list[str]) -> None:
    run = subprocess.run(command, capture_output=True, text=True)
    if run.returncode != 0:
        sys.exit(f"{command[0]} failed:\n{run.stderr}")


def timed(exe: pathlib.Path, rounds: int) -> tuple[float, str]:
    """Minimum wall clock, and what the program printed."""
    best, output = None, None
    for _ in range(rounds):
        start = time.perf_counter()
        run = subprocess.run([str(exe)], capture_output=True, text=True)
        elapsed = time.perf_counter() - start
        if run.returncode != 0:
            sys.exit(f"{exe.name} exited {run.returncode}")
        best = elapsed if best is None else min(best, elapsed)
        output = run.stdout.strip()
    return best, output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rounds", type=int, default=7, help="runs per build")
    args = parser.parse_args()

    cc = next((c for c in ("cc", "clang", "gcc") if shutil.which(c)), None)
    rustc = shutil.which("rustc")
    if cc is None or rustc is None:
        sys.exit("this needs a C compiler and rustc on the path")

    scratch = pathlib.Path(tempfile.mkdtemp(prefix="lex-sys-three-"))
    try:
        # (label, source, how to build it). The lex-sys build is first in
        # each group so the checksum it prints is the one the others are
        # checked against.
        groups = {
            "mandelbrot (compute, Q16.16 fixed point)": [
                ("lex-sys      traps", "mandelbrot.ls", None),
                ("C     -O2    traps", "mandelbrot.c", [cc, "-O2", "-DCHECKED=1"]),
                ("Rust  -O     traps", "mandelbrot.rs", [rustc, "-O", "-Coverflow-checks=on"]),
                ("C     -O2    wraps", "mandelbrot.c", [cc, "-O2", "-DCHECKED=0"]),
                ("Rust  -O     wraps", "mandelbrot.rs", [rustc, "-O", "-Coverflow-checks=off"]),
            ],
            # The row lex-sys cannot fill. Not a fair race against the
            # fixed-point group above and not meant to be: it answers a
            # different question, which is what the *absence* of a float
            # type costs a program that wants this picture.
            "mandelbrot (compute, f64 — lex-sys cannot enter)": [
                ("C     -O2    f64", "mandelbrot_f64.c", [cc, "-O2"]),
                ("Rust  -O     f64", "mandelbrot_f64.rs", [rustc, "-O"]),
            ],
            "sieve (memory-bound, all trapping)": [
                ("lex-sys      traps", "sieve.ls", None),
                ("C     -O2    traps", "sieve.c", [cc, "-O2"]),
                ("Rust  -O     traps", "sieve.rs", [rustc, "-O", "-Coverflow-checks=on"]),
            ],
        }

        for title, builds in groups.items():
            print(f"\n{title}")
            print("-" * max(len(title), 56))
            results, checksums = [], set()
            for index, (label, source, how) in enumerate(builds):
                path = THREE / source
                exe = scratch / f"{title[:4]}{index}"
                if how is None:
                    # `--std` so the benchmark spends its lines on the
                    # kernel rather than on printing an integer.
                    shell([str(compiler()), "build", str(path), "--std", "-o", str(exe)])
                elif how[0] == rustc:
                    shell(how + [str(path), "-o", str(exe)])
                else:
                    shell(how + [str(path), "-o", str(exe)])
                seconds, printed = timed(exe, args.rounds)
                results.append((label, seconds))
                checksums.add(printed)

            if len(checksums) > 1:
                sys.exit(f"the builds disagree: {sorted(checksums)}")

            # Against the C build with the same semantics, which is the
            # first C row in every group.
            reference = next(
                (s for label, s in results if label.startswith("C") and "traps" in label),
                results[0][1],
            )
            for label, seconds in results:
                print(f"{label:20s} {seconds:8.4f}s   {seconds / reference:5.2f}x C")
            print(f"{'':20s} {'':8s}   checksum {checksums.pop()}")
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == "__main__":
    main()
