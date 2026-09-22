# Flags

> **Status: settled and built.**
>
> `ROADMAP.md` has carried *"flag parsing"* under **ordinary work,
> blocked by nothing** for several slices, on the assumption that the
> two programs that parse flags would each be a little shorter with a
> library. That is not what the probe found. They are not too long.
> **They are wrong**, in the same way, and one of them is wrong
> silently. `std.flags` exists because the fix is the same fix twice,
> and because a third program would have written the same guess again.

---

## 1. Six of twelve spellings

`porting.md` calls `examples/base64/` and `examples/cut/` ports of the
GNU programs, "checked against the program it ports". Run each spelling
GNU accepts through both:

| | GNU | ours | |
|---|---|---|---|
| `cut -d, -f2` | `b` | `b` | |
| `cut -f2 -d,` | `b` | `b` | |
| `cut -d, -f1,3` | `a,c` | `a,c` | |
| `cut -d , -f2` | `b` | *usage*, exit 1 | **differs** |
| `cut -d, -f 2` | `b` | *usage*, exit 1 | **differs** |
| `cut --delimiter=, --fields=2` | `b` | *usage*, exit 1 | **differs** |
| `cut -d, -f2 --` | `b` | *usage*, exit 1 | **differs** |
| `base64 -d` | `hi` | `hi` | |
| `base64 -d -i` | `hi` | `hi` | |
| `base64 --` | `YUdrPQo=` | `YUdrPQo=` | |
| `base64 --decode` | `hi` | **`YUdrPQo=`** | **differs, silently** |
| `base64 -di` | `hi` | **`YUdrPQo=`** | **differs, silently** |

Every row above is the state **before** this slice. After it, every row
agrees except the three §4 keeps on purpose, and
`both_ports_match_gnu_on_every_spelling` checks all eighteen against the
real coreutils binaries on every commit.

`cut` fails **loudly**: an unrecognised argument sets its usage flag and
it exits 1 with a message. That is a bad port and an honest program.

`base64` fails **silently**. `--decode` is not two bytes, so the test
`len(flag) == 2 && flag[0] == '-' && flag[1] == 'd'` is false, the flag
is ignored, and the program **encodes its input and exits 0** — the
answer to a question nobody asked, indistinguishable from success. That
is the failure mode this language exists to remove, arrived at through
the one part of a program no type system was watching.

### 1.1 And the tests passed

Every base64 conformance test invokes the program with exactly `-d`.
Twelve input sizes, both directions, three malformed inputs, checked
byte for byte against GNU — through **one spelling of one flag**. The
same shape `line-reading.md` §1 found in `cut`, where the dimension the
test never varied was the length of a line; here it is the spelling of
an argument.

---

## 2. What the syntax actually is

Argument syntax is a specification, not a convention, and the reason
both programs got it wrong is that each reimplemented a guess at it.
What GNU accepts, and what `std.flags` therefore has to:

```
-d              a short flag
-di             bundled: -d then -i
-d,             a short flag's value, attached
-d ,            a short flag's value, as the next argument
--decode        a long flag
--delimiter=,   a long flag's value, after `=`
--delimiter ,   a long flag's value, as the next argument
--              everything after this is positional
-               a positional (conventionally standard input)
file.csv        a positional
```

Nine shapes. `base64` implements one of them and `cut` implements two.

---

## 3. A cursor, not a table

The obvious design is `getopt_long`'s: hand the parser a table of
options and let it drive. It is the wrong shape here, for a reason worth
writing down rather than re-deriving.

A table has to say which options take values, which needs a slice of
`{ name, letter, takes_value }` — expressible — and then, to be GNU,
an **abbreviation resolver**: `cut --del=,` works because `--del` is an
unambiguous prefix of one entry. That is a second search over the table
and a third failure mode, for two callers.

So the parser answers one step at a time and the *program* says when it
wants a value:

```lex-sys
pub enum Step {
    // A short flag's letter: `-d` gives 'd'.
    Short(int),
    // A long flag's name, without the dashes and without any `=value`.
    Long(&static [byte]),
    // An operand: a file, a `-`, or anything after `--`.
    Operand(&static [byte]),
    Done,
}
```

Driven by a `Cursor`, which is a `val` struct of three integers — the
argument index, the offset inside a bundled run, and whether `--` has
been seen — so it copies, needs no region and allocates nothing:

```lex-sys
var c = flags.start();
borrow args as &g in {
    var going = true;
    while going {
        let (next, step) = flags.step(g, c);
        c = next;
        match step {
            Step::Short(letter) => { ... }
            Step::Long(name) => { ... }
            Step::Operand(text) => { ... }
            Step::Done => { going = false; }
        }
    }
}
```

The pair-returning shape is `examples/sort/`'s `(grown, got)` again: no
`&!` on the cursor, because a `val` that is copied and reassigned is
simpler than a reference that has to be borrowed through a `match`.

**A value is asked for, never guessed:**

```lex-sys
let (after, value) = flags.value(g, c);
```

taking the rest of the current argument (`-d,` → `,`, `--delimiter=,` →
`,`) or, if there is none, the whole next argument (`-d ,`). It answers
an empty slice when the arguments run out, which is the one case a
program has to check.

That is the whole interface: `start`, `step`, `value`. A program that
never calls `value` sees every letter as a flag, which is what `base64`
wants; one that calls it after `'d'` sees `-d,` and `-d ,` alike, which
is what `cut` wants.

---

## 4. What it does not do, and one of them is a divergence

* **No abbreviation.** `cut --del=,` works in GNU and is refused here.
  §3 is why: resolving it needs the table this design does not have.
  This is the one place the ports stay unfaithful, and it is now
  **written down** rather than discovered by someone's shell history.
* **No option after an operand**, ever — but neither program needs it,
  and GNU's own permutation is a `POSIXLY_CORRECT` switch away from not
  happening either. `cut -f2 file -d,` is refused here and accepted by
  GNU.
* **No `+flag`**, no optional-argument options, no `--flag=` meaning
  "present but empty" distinct from absent.
* **`base64 -i` and `-w` are now refused**, where before this slice they
  were accepted and ignored. Neither is implementable in this port —
  the decoder refuses garbage and the wrap is fixed at 76 — so accepting
  one is §1's silent lie in a second place. Refusing is a divergence
  from GNU that a user sees; ignoring is one they do not. Implementing
  `-i` is a change to the *decoder* and wants its own tests against GNU
  on garbage input, which is why it is not here.
* **Nothing about what a flag means.** `flags` reports the shape of an
  argument. Whether `-d` is legal, required, or contradicts `-f` is the
  program's, and stays in the program.

---

## 5. The suite

| Test | Shows |
|---|---|
| `both_ports_match_gnu_on_every_spelling` | eighteen rows against the real `/usr/bin/cut` and `/usr/bin/base64`, stdout **and** exit status — including the three §4 keeps as divergences, asserted to still diverge, so a row that quietly starts agreeing is moved rather than forgotten |
| `every_argument_shape` | §2's nine shapes, read back as themselves |
| `tests/accept/flags.ls` | the fixture, which **is** the driver `every_argument_shape` runs: §2's table and the program that prints it are one file |
