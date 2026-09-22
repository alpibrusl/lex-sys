#!/usr/bin/env python3
"""What every check costs a vectoriser, and whether poison costs less.

`docs/overflow-cost.md` §3.2 found that a check costs the *vectoriser*
rather than a branch. It measured the overflow check and wrote "the
check"; `docs/gpu.md` §2.1 falsified half of that -- a bounds check is
free -- and left the rest open. This runs the same experiment over every
check lex-sys emits inside a loop body.

For each kernel in `benches/guards.c` it builds the program three ways —
unchecked, trapping, and `gpu.md` §4.1's **poison**, where the condition
is OR-ed into a flag tested once after the loop — counts the SIMD
instructions in the emitted `run` with `objdump`, and times them
interleaved. Counting the instructions is the point: a timing difference
says something got slower, and only the disassembly says the vectoriser
is what was lost.

    python3 scripts/guards.py                 # baseline x86-64 (SSE2)
    python3 scripts/guards.py --march native  # and with the machine's own
    python3 scripts/guards.py --runs 5

`docs/check-cost.md` is what these numbers mean.
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "benches" / "guards.c"

# Kernel number, name, and what the guard is.
KERNELS = [
    (0, "overflow", "`a + b` traps on overflow"),
    (1, "bounds", "`s[i]` traps outside the slice"),
    (2, "shift", "`a << b` traps outside `0..64`"),
    (3, "byte_of", "`byte_of(n)` traps outside `0..255`"),
    (4, "subslice", "`s[lo..hi]` traps on a bad range -- two tests"),
    (5, "negate", "`-x` traps on the one value with no negation"),
    (6, "float_to_int", "`int_of(f)` traps on NaN and out of range"),
    (7, "divide", "`a / b` traps on a zero divisor -- in the hardware"),
    (8, "subslice_iv", "the same two tests, on the induction variable"),
    (9, "overflow_each", "the same overflow check, element-wise not carried"),
    (10, "overflow_signs", "the same again, as sign logic rather than a builtin"),
    (11, "overflow_carried", "the sign spelling on the reduction of kernel 0"),
]

# Unchecked, trapping, and poison. `gpu.md` §4.1 numbers them the same way.
MODES = (0, 1, 2)

# Two counts, because one is not enough across this range of kernels.
#
# `gpu.md` §2 counted every instruction that touches a vector register.
# That is the right signal for an integer reduction, where nothing else
# uses xmm -- but scalar double arithmetic uses xmm too, so on a float
# kernel it counts a loop that never vectorised. The packed count is the
# one that says *vectorised*; the register count is kept because it is
# what the earlier document reported, and the difference between them
# explains why the same kernel reads 10 there and 8 here: two `movq`
# move the accumulator between a general register and xmm, and they are
# scalar moves in an otherwise packed loop.
VECTOR_REGISTER = re.compile(r"%(?:x|y|z)mm[0-9]+")
# Packed mnemonics: SSE/AVX work whose suffix says how many lanes --
# `p` for packed integer, `...ps`/`...pd` for packed float.
PACKED = re.compile(
    r"^(?:v)?(?:p[a-z0-9]+|movap[sd]|movup[sd]|movdqa|movdqu|"
    r"(?:add|sub|mul|div|min|max|cmp|cvt[a-z0-9]*|sqrt|and|or|xor|blend|shuf|unpck)[a-z0-9]*p[sd])$"
)


def compilers(cc: str) -> str:
    found = shutil.which(cc)
    if not found:
        sys.exit(f"no `{cc}` on the path")
    return found


def build(cc: str, march: str | None, kernel: int, mode: int, out: pathlib.Path) -> None:
    argv = [cc, "-O2", f"-DKERNEL={kernel}", f"-DMODE={mode}"]
    if march:
        argv.append(f"-march={march}")
    argv += [str(SOURCE), "-o", str(out)]
    done = subprocess.run(argv, capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"kernel {kernel} mode={mode} did not build:\n{done.stderr}")


def simd_in_run(exe: pathlib.Path) -> tuple[int, int]:
    """(vector-register instructions, packed instructions) in `run`.

    `run` is the only function that matters, and `noinline` on its
    definition keeps the compiler from dissolving it into `main`.
    """
    done = subprocess.run(
        ["objdump", "-d", "--no-show-raw-insn", str(exe)], capture_output=True, text=True
    )
    if done.returncode != 0:
        sys.exit(f"objdump failed on {exe}")
    inside = False
    registers = 0
    packed = 0
    for line in done.stdout.splitlines():
        if line.endswith("<run>:"):
            inside = True
            continue
        if inside:
            if not line.strip():
                break
            body = line.split("\t", 1)
            if len(body) < 2:
                continue
            text = body[1].strip()
            if not VECTOR_REGISTER.search(text):
                continue
            registers += 1
            words = text.split()
            if words and PACKED.match(words[0]):
                packed += 1
    return registers, packed


def time_once(exe: pathlib.Path) -> tuple[float, str]:
    start = time.perf_counter()
    done = subprocess.run([str(exe)], capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    if done.returncode != 0:
        sys.exit(f"{exe.name} exited {done.returncode} -- a check fired, which is a bug in the data")
    return elapsed, done.stdout.strip()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cc", default="clang")
    ap.add_argument("--march", default=None, help="e.g. native; omitted means baseline x86-64")
    ap.add_argument("--runs", type=int, default=9)
    args = ap.parse_args()
    cc = compilers(args.cc)

    version = subprocess.run([cc, "--version"], capture_output=True, text=True).stdout.splitlines()
    print(version[0] if version else cc)
    print(f"-O2{' -march=' + args.march if args.march else ''}, minimum of {args.runs} runs")
    print("packed SIMD and time are printed unchecked / trap / poison\n")
    print(
        f"{'check':<14}{'packed':>22}{'unchecked':>11}{'trap':>9}{'poison':>9}"
        f"{'trap':>8}{'poison':>9}"
    )
    print("-" * 82)

    rows = []
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        for kernel, name, _ in KERNELS:
            exes, packed, regs, best = {}, {}, {}, {}
            for mode in MODES:
                exes[mode] = tmp / f"{name}_{mode}"
                build(cc, args.march, kernel, mode, exes[mode])
                regs[mode], packed[mode] = simd_in_run(exes[mode])

            # Interleaved across all three, so drift lands on each equally.
            times = {mode: [] for mode in MODES}
            answers = set()
            for _ in range(args.runs):
                for mode in MODES:
                    elapsed, answer = time_once(exes[mode])
                    times[mode].append(elapsed)
                    answers.add(answer)
            if len(answers) != 1:
                sys.exit(f"{name}: the three modes disagree {answers}")
            for mode in MODES:
                best[mode] = min(times[mode]) * 1000

            shape = f"{packed[0]} / {packed[1]} / {packed[2]}"
            print(
                f"{name:<14}{shape:>22}"
                f"{best[0]:>11.1f}{best[1]:>9.1f}{best[2]:>9.1f}"
                f"{best[1] / best[0]:>7.2f}x{best[2] / best[0]:>8.2f}x"
            )
            rows.append((name, regs, packed, best))

    print("\nvector-register counts, which is what `gpu.md` §2 reported:")
    for name, regs, _, _ in rows:
        print(f"  {name:<14}{regs[0]:>4} / {regs[1]} / {regs[2]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
