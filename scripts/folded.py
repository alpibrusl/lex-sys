#!/usr/bin/env python3
"""How much of this repository's code the compiler works out before it runs.

`docs/compile-time.md` §1 is the census this produces. It asks the
compiler rather than re-deriving the answer: `lex-sys authority --output
json` reports `folded_operators` and `folded_calls` for a program, so the
number here is what the pass actually did and not an estimate of what it
could do.

    python3 scripts/folded.py
    python3 scripts/folded.py --quiet

An estimate was what the first draft of §1 had, and it was wrong in both
directions: it counted sites that fold to a single literal as several,
and it missed every operator a folded *call* goes on to expose.
"""

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "target" / "release" / "lex-sys"


def programs():
    """Every single-file program in the repository, in a stable order."""
    found = []
    for directory in ("examples", "tests/accept", "benches/three"):
        found += sorted((ROOT / directory).glob("*.ls"))
    # The multi-file examples are directories; each one's entry point is
    # the file named after it.
    for directory in sorted((ROOT / "examples").iterdir()):
        if directory.is_dir():
            entry = directory / f"{directory.name}.ls"
            if entry.exists():
                found.append(entry)
    return found


def report(path):
    done = subprocess.run(
        [str(BIN), "authority", "--output", "json", "--std", str(path)],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        return None
    return json.loads(done.stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--quiet", action="store_true", help="totals only")
    args = parser.parse_args()

    if not BIN.exists():
        print(f"build it first: cargo build --release   ({BIN} is missing)", file=sys.stderr)
        return 1

    operators = calls = pure = functions = 0
    counted = 0
    for path in programs():
        data = report(path)
        if data is None:
            continue
        counted += 1
        operators += data["folded_operators"]
        calls += data["folded_calls"]
        pure += len(data["pure"])
        functions += data["functions"]
        if not args.quiet and (data["folded_operators"] or data["folded_calls"]):
            print(
                f"{path.relative_to(ROOT).as_posix():<36}"
                f"operators {data['folded_operators']:>3}   calls {data['folded_calls']:>3}"
            )

    print()
    print(f"{counted} programs")
    print(f"  operators evaluated   {operators}")
    print(f"  calls evaluated       {calls}")
    print(f"  provably pure         {pure} of {functions}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
