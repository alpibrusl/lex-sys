//~ ERROR no module `nowhere.at.all` in this program
//~ RULE unknown-name

// `docs/modules.md` §4.
//
// Checked at the `import` rather than where a name is used, because an
// import naming nothing is wrong whether or not anything reached through
// it -- and an *unused* wrong import is exactly the one a programmer
// wants told about. Nothing below mentions `nowhere`.
//
// "In this program" is the whole rule: a module exists where a file
// declares it, and the program is the set of files on the command line
// (`many-files.md`). There is no search path to get wrong.

import nowhere.at.all;

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args);
    release(ffi);
    release(fs);
    release(heap);
    release(io);
    return 0;
}
