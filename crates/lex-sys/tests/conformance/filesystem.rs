//! `Fs(prefix)`: paths, files and handles, checked against the prefix they were granted.

use super::*;

#[test]
fn a_path_outside_the_granted_prefix_traps() {
    // `docs/filesystem.md` §4. The prefix lives in the type and is known at
    // compile time; the path is a runtime slice, because a program that
    // could not name a file at run time could not be a tool. So the check
    // happens where the path is, and a path outside what the capability
    // granted *traps* -- it is not a missing file, it is a program doing
    // something its own type said it would not.
    let dir = scratch("fs-outside-prefix");
    let source = dir.join("outside.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-granted\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/etc/hostname\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("outside");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a path outside the prefix should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_path_containing_dot_dot_traps() {
    // §4.1. A prefix check on bytes is defeated by `/tmp/../etc/passwd`,
    // and there are two honest answers: normalise the path, or refuse it.
    // Normalisation is a security function with a long history of being got
    // wrong and needs its own design, symlinks included -- so M3 refuses,
    // visibly, rather than shipping a check that quietly does not hold.
    let dir = scratch("fs-dot-dot");
    let source = dir.join("traversal.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/../etc/hostname\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("traversal");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a path containing `..` should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sibling_of_the_granted_directory_traps() {
    // §1, at run time this time. `/tmp/lex-sys-granted` does not contain
    // `/tmp/lex-sys-granted-elsewhere`, however many bytes the two names
    // share. The compile-time refusal (`tests/reject/fs_sibling_prefix.ls`)
    // covers the same rule for the *prefix*; this covers it for the path,
    // which is the half nobody can see before the program runs.
    let dir = scratch("fs-sibling");
    let source = dir.join("sibling.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-granted\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/lex-sys-granted-elsewhere\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("sibling");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a sibling of the granted directory should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_written_file_is_readable_by_its_owner() {
    // The regression guard for a real bug, and the reason it is worth a test
    // of its own rather than leaving it to `file_roundtrip.ls`.
    //
    // `open` is variadic -- `int open(const char *, int, ...)` -- and on
    // Apple ARM64 a variadic argument travels on the stack while a fixed one
    // travels in a register. Calling it with three *fixed* arguments
    // therefore created files with whatever mode happened to be on the
    // stack: the write succeeded and reported the right byte count, and the
    // file was unreadable afterwards. Linux x86-64 cannot see this, because
    // there varargs and fixed arguments share the same registers.
    //
    // So the mode is checked directly, from outside the program, rather than
    // inferred from a read that happens to succeed.
    let dir = scratch("fs-mode");
    let source = dir.join("mode.ls");
    let target = dir.join("written.txt");
    let path = target.to_string_lossy().into_owned();
    std::fs::write(
        &source,
        format!(
            "fn main(world: World) -> [] int {{\n\
                 let Split {{ io, ffi, fs, heap, args }} = split(world); release(args); release(heap); release(ffi); release(io);\n\
                 let one = narrow(fs, \"{path}\");\n\
                 var wrote = 0;\n\
                 borrow one as &f in {{ wrote = fs_write(f, \"{path}\", \"written\\n\"); }}\n\
                 release(one);\n\
                 return wrote - 8;\n\
             }}\n"
        ),
    )
    .expect("a writable fixture");

    let exe = dir.join("mode");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(0), "the write should have reported 8 bytes");

    let written = std::fs::read(&target).expect("the file the program wrote is readable");
    assert_eq!(written, b"written\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&target).expect("the file exists").permissions().mode();
        // `creat` asks for 0644 and the umask may clear group and other
        // bits, but never the owner's. A mode that lost them is the bug.
        assert_eq!(mode & 0o600, 0o600, "created with mode {:o}", mode & 0o777);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_minus_one_rather_than_a_trap() {
    // §3. The distinction `defined-behaviour.md` draws everywhere: `-1` for
    // an outcome a program should handle, a trap for a broken promise. A
    // file that is not there is the first kind.
    let dir = scratch("fs-missing");
    let source = dir.join("missing.ls");
    std::fs::write(
        &source,
        "fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args } = split(world); release(args); release(heap); release(ffi); release(io);\n\
             let tmp = narrow(fs, \"/tmp/lex-sys-not-here\");\n\
             var read = 0;\n\
             region a {\n\
                 let buffer = alloc_slice[a](16, byte_of(0));\n\
                 borrow tmp as &f in {\n\
                     read = fs_read(f, \"/tmp/lex-sys-not-here/at-all\", buffer);\n\
                 }\n\
             }\n\
             release(tmp);\n\
             return 0 - read;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("missing");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert_eq!(run.status.code(), Some(1), "a missing file should return -1, not trap");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/file-handles.md` §4.1 — the prefix survives the rewrite.
///
/// `bulk-io.md` §3.2's rule is that a program must not look more
/// powerful for having been written better, and the test there pins the
/// authority report **byte for byte**: `write_bytes` writes the same
/// stream `putchar` writes, so a new label would have been a regression.
///
/// Here the rule needs one more word, and the difference is real rather
/// than a loophole. A handle program *does* perform something the path
/// program does not — it holds a descriptor across statements — and §4
/// chose to say so, in a label carrying **no argument**. So the test is
/// not byte-identity but the thing §3.2 was actually protecting:
///
///   * every label that carries an argument is identical, so the
///     directory is still named and named the same way;
///   * the handle program adds exactly one label, and it has no
///     argument to widen.
///
/// If `read` ever grew a path — §4's first option, which would make a
/// handle unusable by a function that was not told where it came from —
/// the first assertion catches it.
#[test]
fn a_handle_reports_the_same_prefix_a_path_does() {
    let prologue = "\
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(args); release(heap); release(ffi); release(io);
";
    let whole_file = format!(
        "{prologue}    region a {{\n        \
         var buffer = alloc_slice[a](8, byte_of(0));\n        \
         borrow fs as &c in {{ fs_read(c, \"/tmp/lex-sys-authority.txt\", buffer); }}\n    \
         }}\n    release(fs);\n    return 0;\n}}\n"
    );
    let handle = format!(
        "{prologue}    region a {{\n        \
         var buffer = alloc_slice[a](8, byte_of(0));\n        \
         borrow fs as &c in {{\n            \
         match open_read(c, \"/tmp/lex-sys-authority.txt\") {{\n                \
         Opened::Ok(f) => {{\n                    var file = f;\n                    \
         borrow mut file as &!h in {{ file_read(h, buffer); }}\n                    \
         file_close(file);\n                }}\n                \
         Opened::Failed(e) => {{ }}\n            }}\n        }}\n    }}\n    \
         release(fs);\n    return 0;\n}}\n"
    );

    let by_path = authority_json(&whole_file, "handle-authority-path");
    let by_handle = authority_json(&handle, "handle-authority-handle");

    // The labels that name something. Every label has an `"argument"` key;
    // a path-free one spells it `null`, which is exactly the difference
    // this test is about.
    let arguments = |json: &str| {
        json.match_indices("{ \"name\":")
            .filter_map(|(at, _)| json[at..].find('}').map(|end| json[at..at + end + 1].to_owned()))
            .filter(|row| !row.contains("\"argument\": null"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        arguments(&by_path),
        arguments(&by_handle),
        "a handle must name the same directory a path does (§4.1):\n{by_path}\n{by_handle}"
    );
    assert!(
        by_path.contains("\"fs_read\""),
        "the path program should name the directory at all:\n{by_path}"
    );

    // And the one label the rewrite adds carries nothing to widen.
    assert!(
        by_handle.contains("\"file_read\""),
        "a handle program performs `file_read` (§4.1):\n{by_handle}"
    );
    assert!(
        by_handle.contains("{ \"name\": \"file_read\", \"argument\": null, \"bounded\": true }"),
        "`file_read` must carry no argument, or a handle would need its own path:\n{by_handle}"
    );
    assert!(!by_path.contains("file_read"), "the path program never opens a handle:\n{by_path}");
}
