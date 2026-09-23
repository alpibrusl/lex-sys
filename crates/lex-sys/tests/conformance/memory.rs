//! Where values live: the heap, `static` data, and what a type costs.

use super::*;

#[test]
fn the_heap_actually_frees() {
    // `docs/heap.md` §3.1 claims the general heap cannot leak. The checker
    // guarantees `unbox` runs on every path, but that is a claim about the
    // *program* -- this is the claim about the emitted code.
    //
    // Eight million boxes of 2 KiB each, one at a time. Freeing makes the
    // footprint one box; leaking makes it 16 GB, which no machine this runs
    // on has. So a regression that dropped the `free` does not produce a
    // subtly worse number here, it fails: either our own `trapz` fires when
    // `malloc` returns null, or the process is killed. Both are a non-zero
    // exit, and both are what this asserts against.
    //
    // (Run under valgrind on linux-x86_64 while this was written: one
    // million allocs, one million frees, "in use at exit: 0 bytes in 0
    // blocks". Valgrind is not on both CI targets, so the portable check is
    // the one above.)
    const FIELDS: usize = 256;
    const ROUNDS: usize = 8_000_000;

    let fields = (0..FIELDS).map(|i| format!("f{i}: int")).collect::<Vec<_>>().join(", ");
    let init = (0..FIELDS).map(|i| format!("f{i}: 1")).collect::<Vec<_>>().join(", ");

    let dir = scratch("heap-frees");
    let source = dir.join("churn.ls");
    std::fs::write(
        &source,
        format!(
            "struct Chunk {{ {fields} }}\n\
             fn churn[&h](heap: &!h Heap, rounds: int) -> [heap] int {{\n\
                 var total = 0;\n\
                 var i = 0;\n\
                 while i < rounds {{\n\
                     let b = box(heap, Chunk {{ {init} }});\n\
                     let c = unbox(heap, b);\n\
                     total = total + c.f0;\n\
                     i = i + 1;\n\
                 }}\n\
                 return total;\n\
             }}\n\
             fn main(world: World) -> [] int {{\n\
                 let Split {{ io, ffi, fs, heap, args }} = split(world); release(args);\n\
                 release(ffi); release(fs); release(io);\n\
                 var total = 0;\n\
                 borrow mut heap as &!h in {{ total = churn(h, {ROUNDS}); }}\n\
                 release(heap);\n\
                 return total - {ROUNDS};\n\
             }}\n"
        ),
    )
    .expect("a writable fixture");

    let exe = dir.join("churn");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "eight million boxes in a bounded footprint should succeed; a leak would need 16 GB"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/compile-time-data.md` §1.1 — the row that is a capability
/// argument rather than a convenience.
///
/// An arena is one 64 KiB chunk and exhausting it traps, so a
/// 65 536-entry `[int]` table — 512 KB, the shape a CRC or a 16-bit codec
/// uses — cannot be built in a `region` at all. A program that released
/// `heap` therefore cannot have one. A `static` has no such ceiling,
/// because the data is in the file rather than in a chunk.
///
/// Both halves are checked here, because the claim is a comparison: the
/// `region` version must trap and the `static` version must work.
#[test]
fn a_static_outgrows_what_an_arena_could_hold() {
    let body = "\
    var i = 0;\n\
    while i < 65536 {\n\
        table[i] = i * 3;\n\
        i = i + 1;\n\
    }\n";

    let with_static = format!(
        "fn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
         \x20   if len(big) != 65536 {{ return 1; }}\n\
         \x20   return big[65535] - 196605;\n\
         }}\n\
         static big: [int] {{\n\
         \x20   let table = alloc_slice[static](65536, 0);\n\
         {body}\
         \x20   return table;\n\
         }}\n"
    );
    let with_region = format!(
        "fn main(world: World) -> [] int {{\n\
         \x20   let Split {{ io, ffi, fs, heap, args }} = split(world);\n\
         \x20   release(args); release(heap); release(fs); release(ffi); release(io);\n\
         \x20   region a {{\n\
         \x20       let table = alloc_slice[a](65536, 0);\n\
         {body}\
         \x20       if table[65535] != 196605 {{ return 1; }}\n\
         \x20   }}\n\
         \x20   return 0;\n\
         }}\n"
    );

    let dir = scratch("static-big");
    for (name, source, should_run) in
        [("static", with_static, true), ("region", with_region, false)]
    {
        let path = dir.join(format!("{name}.ls"));
        std::fs::write(&path, &source).expect("a writable fixture");
        let exe = dir.join(name);
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(
            build.status.success(),
            "`{name}` should compile — the arena's limit is a run-time one:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new(&exe).output().expect("the program runs");
        if should_run {
            assert_eq!(
                run.status.code(),
                Some(0),
                "a 512 KB `static` is data in the binary, so there is no chunk to exhaust"
            );
        } else {
            assert_eq!(
                run.status.code(),
                None,
                "512 KB in a 64 KiB arena traps, which is the whole of §1.1's second row"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// §2 — a `static` is read-only data, and the program never builds it.
///
/// Checked through the authority report rather than a disassembler, for
/// the reason `compile-time.md` §9 gives: CI builds on two platforms and
/// `objdump` is not among the things they share. A program whose only
/// arithmetic is inside a `static` performs nothing and folds nothing at
/// run time, which is what "the loop ran in the compiler" looks like from
/// outside.
#[test]
fn a_static_needs_no_authority_and_no_heap() {
    let source = "\
static table: [int] {
    let t = alloc_slice[static](8, 0);
    var i = 0;
    while i < 8 { t[i] = i * i; i = i + 1; }
    return t;
}
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io);
    return table[3] - 9;
}
";
    let json = authority_json(source, "static-authority");
    assert!(json.contains("\"effects\": []"), "a `static` performs nothing:\n{json}");
    assert!(json.contains("\"foreign_symbols\": []"), "and reaches no foreign code:\n{json}");

    let dir = scratch("static-runs");
    let path = dir.join("static.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("static");
    let build = Command::new(BIN)
        .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert_eq!(run.status.code(), Some(0), "`table[3]` is 9, computed during compilation");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/layout.md` §4 — the report agrees with what the backend emits.
///
/// The check that matters is `stride`, because that is the number a
/// program can *observe*: an arena is a fixed 64 KiB and exhausting it
/// traps, so how many elements fit is exactly `65536 / stride`. A report
/// that drifted from the emitter would disagree with where the trap
/// lands, and this finds it by walking the boundary from both sides.
#[test]
fn the_layout_report_says_what_a_type_costs() {
    let dir = scratch("layout-report");
    let source = "\
struct Rgb { r: byte, g: byte, b: byte }
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(fs); release(ffi); release(io);
    region a {
        let s = alloc_slice[a](COUNT, Rgb { r: byte_of(1), g: byte_of(2), b: byte_of(3) });
        if len(s) == 0 { return 1; }
    }
    return 0;
}
";
    let path = dir.join("rgb.ls");
    std::fs::write(&path, source.replace("COUNT", "1")).expect("a writable fixture");

    let report = Command::new(BIN)
        .args(["layout".as_ref(), path.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(report.status.success(), "{}", String::from_utf8_lossy(&report.stderr));
    let text = String::from_utf8_lossy(&report.stdout);

    let row = text
        .lines()
        .find(|l| l.starts_with("Rgb"))
        .unwrap_or_else(|| panic!("no `Rgb` row in:\n{text}"));
    let columns: Vec<u32> =
        row.split_whitespace().skip(1).map(|n| n.parse().expect("a number")).collect();
    assert_eq!(columns[0], 3, "three leaves:\n{text}");
    assert_eq!(columns[1], 24, "eight bytes each today:\n{text}");
    assert_eq!(columns[2], 3, "one byte each packed — §2's whole point:\n{text}");
    let stride = columns[3];
    assert_eq!(stride, 24, "and the stride is what a traversal pays:\n{text}");

    // Now the observable half: an arena is 64 KiB, so `65536 / stride`
    // elements fit and one more does not.
    let fits = 65536 / stride;
    for (count, should_run) in [(fits, true), (fits + 1, false)] {
        let path = dir.join(format!("rgb{count}.ls"));
        std::fs::write(&path, source.replace("COUNT", &count.to_string()))
            .expect("a writable fixture");
        let exe = dir.join(format!("rgb{count}"));
        let build = Command::new(BIN)
            .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
            .output()
            .expect("the compiler runs");
        assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
        let run = Command::new(&exe).output().expect("the program runs");
        if should_run {
            assert_eq!(
                run.status.code(),
                Some(0),
                "{count} × {stride} bytes is exactly one arena, so it fits"
            );
        } else {
            assert_eq!(
                run.status.code(),
                None,
                "one element past the arena traps, which is how the stride is observable"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
