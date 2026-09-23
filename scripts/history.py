#!/usr/bin/env python3
"""Replay this repository's own past through today's compiler.

`docs/editions.md` §2 is what these numbers mean. Every distinct revision
of every `.ls` file under `std/` and `examples/` is checked by one
binary, today's, so anything that fails is the language having moved.

A library file is checked beside today's other library files and a
`main` that does nothing, because a file of a program is not a program.
An example in a directory is checked beside its siblings as they were
in the commit that introduced it.

    python3 scripts/history.py             # replay, and classify what fails
    python3 scripts/history.py --migrate   # then try the two mechanical steps
    python3 scripts/history.py --alias     # or read an old `io` as both halves

`--alias` is what an edition could do inside the compiler without a
tool: read an old file's `io` label as `io_read, io_write`, the two labels
that replaced it, and check again.

`--migrate` simulates a migration tool driven by the checker's own
refusals, applied only to the file under test. It knows two steps:

* a row repair: add a label the body performs, drop one it does not
  (and drop the old `io` whenever one of its halves is added);
* a `Split` repair: name the capabilities the pattern leaves out, and
  release each at once.

Anything else stops it, so what it recovers is a lower bound on what a
real tool, one that edits every file of a program, would recover.
"""

import argparse
import collections
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "target" / "release" / "lex-sys"
FIELDS = ["io", "ffi", "fs", "heap", "args"]
EMPTY_MAIN = (
    "fn main(world: World) -> [] int {\n"
    "    let Split { io, ffi, fs, heap, args } = split(world);\n"
    "    release(args);\n    release(ffi);\n    release(fs);\n"
    "    release(heap);\n    release(io);\n    return 0;\n}\n"
)


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True).stdout


def revisions():
    """(blob, path) -> the oldest commit that has it."""
    seen = {}
    for commit in reversed(git("log", "--format=%H", "--", "std", "examples").split()):
        for line in git("ls-tree", "-r", commit, "--", "std", "examples").splitlines():
            meta, path = line.split("\t")
            blob = meta.split()[2]
            if path.endswith(".ls") and (blob, path) not in seen:
                seen[(blob, path)] = commit
    return seen


def refusals(files, args):
    out = subprocess.run(
        [str(BIN), "check", "--output", "json", *map(str, files), *args],
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)["refused"]


def lay_out(work, blob, path, commit):
    """Write the program a revision belongs to; answer (files, target, args)."""
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    files = []

    def put(name, text):
        p = work / name
        p.write_text(text)
        files.append(p)
        return p

    target = put("target_" + os.path.basename(path), git("cat-file", "-p", blob))
    if path.startswith("std/"):
        for lib in sorted((ROOT / "std").glob("*.ls")):
            if lib.name != os.path.basename(path):
                put("std_" + lib.name, lib.read_text())
        put("main.ls", EMPTY_MAIN)
        return files, target, []
    folder = os.path.dirname(path)
    if folder != "examples":
        for line in git("ls-tree", commit, folder + "/").splitlines():
            meta, sibling = line.split("\t")
            if sibling.endswith(".ls") and sibling != path:
                put("sib_" + os.path.basename(sibling), git("cat-file", "-p", meta.split()[2]))
    return files, target, ["--std"]


def repair(text, refusal, target):
    """One migration step on the file under test, or None."""
    pos = refusal.get("position")
    if not pos or not pos["file"].endswith(target.name):
        return None
    lines = text.split("\n")
    line, col = pos["line"] - 1, pos["column"] - 1
    msg = refusal["message"]
    if refusal["rule"] == "type-mismatch" and msg.startswith("expected `[`"):
        lines[line] = lines[line][:col] + "[] " + lines[line][col:]
        return "\n".join(lines)
    if msg.startswith("`Split` has") and "this pattern names" in msg:
        m = re.search(r"Split\s*\{([^}]*)\}", lines[line])
        if not m:
            return None
        have = [f.strip() for f in m.group(1).split(",") if f.strip()]
        missing = [f for f in FIELDS if f not in have]
        indent = re.match(r"\s*", lines[line]).group(0)
        lines[line] = lines[line][: m.start()] + "Split { " + ", ".join(have + missing) + " }" + lines[line][m.end():]
        for f in reversed(missing):
            lines.insert(line + 1, f"{indent}release({f});")
        return "\n".join(lines)
    added = re.match(r"`[^`]*` performs `([^`]*)`, which its row", msg)
    dropped = re.match(r"`[^`]*` declares `([^`]*)` but never performs it", msg)
    if not (added or dropped):
        return None
    for k in range(line, min(line + 6, len(lines))):
        m = re.search(r"->\s*\[([^\]]*)\]", lines[k])
        if m:
            labels = [l.strip() for l in m.group(1).split(",") if l.strip()]
            if added:
                labels = [l for l in labels if l != "io"] + [added.group(1)]
            else:
                labels = [l for l in labels if l != dropped.group(1)]
            labels = list(dict.fromkeys(labels))
            lines[k] = lines[k][: m.start()] + "-> [" + ", ".join(labels) + "]" + lines[k][m.end():]
            return "\n".join(lines)
    return None


def shape(message):
    message = re.sub(r"`[^`]*`", "`_`", message)
    return re.sub(r"\[[^\]]*\]", "[_]", message)[:100]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--migrate", action="store_true")
    parser.add_argument("--alias", action="store_true")
    options = parser.parse_args()
    if not BIN.exists():
        sys.exit("build first: cargo build --release")

    work = pathlib.Path(tempfile.mkdtemp(prefix="lex-sys-history-"))
    failing = collections.Counter()
    recovered = collections.Counter()
    remaining = collections.Counter()
    total = 0
    for (blob, path), commit in sorted(revisions().items(), key=lambda kv: kv[0][1]):
        total += 1
        files, target, args = lay_out(work, blob, path, commit)
        first = refusals(files, args)
        if not first:
            continue
        failing[(first[0]["rule"], shape(first[0]["message"]))] += 1
        if options.alias:
            text = re.sub(
                r"->\s*\[([^\]]*)\]",
                lambda m: "-> [" + ", ".join(dict.fromkeys(
                    x for l in m.group(1).split(",") if l.strip()
                    for x in (["io_read", "io_write"] if l.strip() == "io" else [l.strip()])
                )) + "]",
                target.read_text(),
            )
            target.write_text(text)
            now = refusals(files, args)
            if now:
                remaining[(now[0]["rule"], shape(now[0]["message"]))] += 1
            else:
                recovered[first[0]["rule"]] += 1
            continue
        if not options.migrate:
            continue
        for _ in range(60):
            now = refusals(files, args)
            if not now:
                break
            step = next((r for r in (repair(target.read_text(), x, target) for x in now) if r), None)
            if step is None:
                break
            target.write_text(step)
        now = refusals(files, args)
        if now:
            remaining[(now[0]["rule"], shape(now[0]["message"]))] += 1
        else:
            recovered[first[0]["rule"]] += 1
    shutil.rmtree(work, ignore_errors=True)

    unreadable = sum(failing.values())
    print(f"{total} revisions, {total - unreadable} read today, {unreadable} do not")
    for (rule, message), n in failing.most_common():
        print(f"  {n:4}  {rule:24} {message}")
    if options.migrate or options.alias:
        how = "the two mechanical steps" if options.migrate else "reading `io` as both halves"
        print(f"\nrecovered by {how}: {sum(recovered.values())}")
        print("what stops the rest:")
        for (rule, message), n in remaining.most_common():
            print(f"  {n:4}  {rule:24} {message}")


if __name__ == "__main__":
    main()
