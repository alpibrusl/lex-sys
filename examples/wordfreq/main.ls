// `wordfreq` — word frequencies, and the capstone of M3.
//
// Every capability the language has is here and each one is doing real
// work, not being demonstrated:
//
//   * **more than one file** (`docs/many-files.md`) — `text.ls` holds the
//     byte helpers, `counts.ls` the tally, and neither knows about the
//     other's job. Before this there was no such thing as a library;
//   * **arguments** (`docs/arguments.md`) — given a path it counts that
//     file, given nothing it counts the sample below;
//   * **file IO** (`docs/filesystem.md`) — reached through an `Fs`, with
//     the row saying which paths;
//   * **the heap** (`docs/heap.md`) — the tally grows as the document is
//     read, which a block-scoped lifetime cannot express, and every node
//     is freed because a `Box` is `res`;
//   * **reading through references** (`docs/reading-references.md`) —
//     `bump` increments a count *in place* through a unique reference,
//     and `report` reads the whole list without spending it;
//   * **slices and strings** — a word is a range in the document, never a
//     copy.
//
// Run it:
//
//     lex-sys build examples/wordfreq/main.ls examples/wordfreq/text.ls \
//                   examples/wordfreq/counts.ls -o wordfreq
//     ./wordfreq            # the sample
//     ./wordfreq notes.txt  # a file
//
// The list is built by prepending, so the output is in reverse
// first-seen order.

// Counted by scanning `text[0 .. length]` for runs of non-space bytes.
// Each run is a word: found in the tally, it is incremented in place;
// otherwise it is added, which is the only part that allocates.
fn tally_words[&h, &t](heap: &!h Heap, text: &t [byte], length: int) -> [heap] Counts {
    var counts = Counts::Empty;
    var at = 0;
    while at < length {
        // Skip whatever separates the last word from this one.
        while at < length && is_space(text[at]) {
            at = at + 1;
        }
        let start = at;
        while at < length && is_space(text[at]) == false {
            at = at + 1;
        }
        let size = at - start;
        if size > 0 {
            var seen = false;
            borrow mut counts as &!c in {
                seen = bump(c, text, start, size);
            }
            if seen == false {
                counts = add(heap, counts, start, size);
            }
        }
    }
    return counts;
}

fn run[&h, &f, &g, &i](
    heap: &!h Heap,
    fs: &f Fs(""),
    args: &g Args,
    io: &!i Io,
) -> [args, fs_read(""), fs_write(""), heap, io] int {
    var distinct = 0;
    region a {
        let text = alloc_slice[a](4096, byte_of(0));

        // Given a path, count that file; given nothing, lay down the
        // sample and count it. `lines.ls` explains why `fs` is passed on
        // unnarrowed when the path comes from the user.
        var length = 0;
        if arg_count(args) > 1 {
            length = fs_read(fs, arg(args, 1), text);
            if length < 0 {
                write_all(io, "cannot read that file\n");
                return 1;
            }
        } else {
            let sample = "the quick brown fox jumps over the lazy dog the fox\n";
            fs_write(fs, "/tmp/lex-sys-wordfreq.txt", sample);
            length = fs_read(fs, "/tmp/lex-sys-wordfreq.txt", text);
        }

        let counts = tally_words(heap, text, length);

        // Read without spending, then spend exactly once.
        borrow counts as &c in {
            distinct = report(c, text, io);
        }
        release_counts(heap, counts);
    }
    return distinct;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(ffi);

    var status = 0;
    borrow mut heap as &!h in {
        borrow fs as &f in {
            borrow args as &g in {
                borrow mut io as &!i in {
                    status = run(h, f, g, i);
                }
            }
        }
    }
    release(heap);
    release(fs);
    release(args);
    release(io);
    return status - 8;
}
