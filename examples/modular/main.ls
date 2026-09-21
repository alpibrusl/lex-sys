//~ STDOUT seen 3, total 60
//~ STDOUT 60

// `main` lives in the **root** module, which needs no declaration and
// cannot be imported (`docs/modules.md` §5.1): the root sees out, and
// nothing sees in.
//
// Two imports, one renamed. `counts` is bound by its last segment and
// `fmt.text` is bound as `fmt` here -- a qualifier is a name this file
// chose, not a path, which is why the same module can be `text` in
// `counts.ls` and `fmt` here without either being wrong.
//
// Run it:
//
//     lex-sys build examples/modular/main.ls examples/modular/counts.ls \
//         examples/modular/text.ls -o modular
//     ./modular

import fmt.counts;
import fmt.text as fmt;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);

    var total = 0;
    borrow mut io as &!i in {
        // A qualified type, in a pattern and in an annotation.
        let t: counts.Tally = counts.empty();
        let t = counts.add(t, 10);
        let t = counts.add(t, 20);
        let t = counts.add(t, 30);
        total = counts.report(i, t);

        fmt.print_nat(i, total);
        fmt.newline(i);
    }
    release(io);
    return total - 60;
}
