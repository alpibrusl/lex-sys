// reverse-complement, from the Computer Language Benchmarks Game.
//
// Reads FASTA records from stdin and prints each one's header followed
// by its sequence reversed and complemented, wrapped at 60 columns.
// `docs/bulk-io.md` §3.3 is why the read side is `getchar`, one byte at
// a time, and not the buffered read the Game's own entry uses: there is
// no bulk `read` here, on purpose, because it needs the three-way
// short-read/end-of-input/error answer `filesystem.md` §3 defers. The
// write side is bulk, the same 60-byte prepared line `fasta.ls` uses.
//
// Checked against the output `fasta.c` prints at N=1000 -- which is also
// the Benchmarks Game's own N=1000 test input for this program -- fed
// back through this one and compared byte for byte with the Game's own
// reference output (`crates/lex-sys/tests/conformance/benchmarks.rs`,
// `benches/game/fasta-1000.txt`, `benches/game/revcomp-1000.txt`).
//
// The complement table is IUPAC ambiguity codes both ways, upper and
// lower case mapping to the same upper-case output: that case-folding is
// the reference implementation's own choice, not a rule this program
// invented, and it is what makes `TWO`'s and `THREE`'s mostly-lowercase
// input come back upper-case.
//
//~ EXIT 0

import std.buffer;
import std.io;

// One (source, complement) pair, both cases of the source mapping to the
// same upper-case complement.
fn pair[&c](table: &!c [byte], base: int, comp: byte) -> [] int {
    table[base] = comp;
    // 32 is the ASCII distance between an upper-case letter and its
    // lower-case twin -- `bytes.to_lower` takes an `int` classified
    // first, and every `base` here already is one.
    table[base + 32] = comp;
    return 0;
}

fn build_complement[&c](table: &!c [byte]) -> [] int {
    pair(table, 'A', byte_of('T'));
    pair(table, 'C', byte_of('G'));
    pair(table, 'G', byte_of('C'));
    pair(table, 'T', byte_of('A'));
    pair(table, 'U', byte_of('A'));
    pair(table, 'M', byte_of('K'));
    pair(table, 'K', byte_of('M'));
    pair(table, 'R', byte_of('Y'));
    pair(table, 'Y', byte_of('R'));
    pair(table, 'W', byte_of('W'));
    pair(table, 'S', byte_of('S'));
    pair(table, 'V', byte_of('B'));
    pair(table, 'B', byte_of('V'));
    pair(table, 'H', byte_of('D'));
    pair(table, 'D', byte_of('H'));
    pair(table, 'N', byte_of('N'));
    return 0;
}

// The header, as read, then the sequence reversed and complemented,
// wrapped at 60 columns. Read-only over both buffers: the caller still
// owns them and clears them for the next record.
fn flush_record[&i, &c, &l, &hb, &sb](
    out: &!i Io,
    complement: &c [byte],
    line: &!l [byte],
    header: &hb buffer.Buffer,
    seq: &sb buffer.Buffer,
) -> [io_write] int {
    io.write_all(out, buffer.bytes(header));
    io.newline(out);

    let body = buffer.bytes(seq);
    let total = len(body);
    var col = 0;
    var k = 0;
    while k < total {
        let src = body[total - 1 - k];
        line[col] = complement[int_of(src)];
        col = col + 1;
        if col == 60 {
            io.write_all(out, line);
            col = 0;
        }
        k = k + 1;
    }
    if col != 0 {
        io.write_all(out, line[0..col]);
        io.newline(out);
    }
    return 0;
}

fn run[&h, &i](heap: &!h Heap, term: &!i Io) -> [heap, io_read, io_write] int {
    region a {
        let complement = alloc_slice[a](128, byte_of(0));
        build_complement(complement);

        let line = alloc_slice[a](61, byte_of(0));
        line[60] = byte_of('\n');

        var header = buffer.empty(heap, 128);
        var seq = buffer.empty(heap, 4096);
        var have_record = false;
        var in_header = false;

        var c = getchar(term);
        while c >= 0 {
            if in_header {
                if c == '\n' {
                    in_header = false;
                } else {
                    header = buffer.push(heap, header, byte_of(c));
                }
            } else {
                if c == '>' {
                    if have_record {
                        borrow header as &hb in {
                            borrow seq as &sb in {
                                flush_record(term, complement, line, hb, sb);
                            }
                        }
                        borrow mut header as &!hbm in {
                            buffer.clear(hbm);
                        }
                        borrow mut seq as &!sbm in {
                            buffer.clear(sbm);
                        }
                    }
                    have_record = true;
                    header = buffer.push(heap, header, byte_of(c));
                    in_header = true;
                } else {
                    if c != '\n' {
                        seq = buffer.push(heap, seq, byte_of(c));
                    }
                }
            }
            c = getchar(term);
        }
        if have_record {
            borrow header as &hb in {
                borrow seq as &sb in {
                    flush_record(term, complement, line, hb, sb);
                }
            }
        }

        buffer.drop(heap, header);
        buffer.drop(heap, seq);
    }
    return 0;
}

fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(fs);
    release(ffi);
    release(args);
    borrow mut heap as &!h in {
        borrow mut io as &!i in {
            run(h, i);
        }
    }
    release(heap);
    release(io);
    return 0;
}
