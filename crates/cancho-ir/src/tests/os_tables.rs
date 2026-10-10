//! The constants a backend writes into a program for the file and directory
//! builtins, per operating system (`docs/wasm.md`).
//!
//! WASI's are read off wasi-libc's own headers (`__header_fcntl.h`,
//! `__struct_stat.h`, `__struct_dirent.h`, `__header_dirent.h`,
//! `__errno_values.h`) -- not guessed -- and two of them contradict what this
//! file used to say about every target: `O_RDONLY` is not zero, and `d_type`'s
//! values are not shared.

use crate::{
    DirentTypes, OpenMode, Os, WASI_ERRNO_TO_LINUX, dirent_layout_for, dirent_types,
    enametoolong_for, linux_errno_from_wasi, open_flags, open_flags_for, stat_layout,
    stat_layout_for,
};

#[test]
fn wasi_open_flags_are_wasi_libcs() {
    let f = open_flags_for(Os::Wasi, false);
    assert_eq!(
        f.read_only, 0x0400_0000,
        "O_RDONLY is a real flag on WASI; zero asks for no rights"
    );
    assert_eq!(f.write_only, 0x1000_0000);
    assert_eq!((f.create, f.directory, f.exclusive, f.truncate), (0x1000, 0x2000, 0x4000, 0x8000));
    assert_eq!((f.append, f.nofollow), (0x1, 0x0100_0000));
    assert_eq!(f.cloexec, 0, "a WASI module cannot exec, so O_CLOEXEC is 0");
    assert_eq!(f.at_fdcwd, -2);
}

#[test]
fn a_read_only_open_is_a_no_op_off_wasi() {
    for (os, aarch64) in [(Os::Linux, false), (Os::Linux, true), (Os::Darwin, false)] {
        assert_eq!(open_flags_for(os, aarch64).read_only, 0, "{os:?}");
    }
}

#[test]
fn the_bool_helpers_still_answer_the_hosts_they_always_did() {
    // Cranelift calls these; nothing about it moved.
    assert_eq!(open_flags(false, false), open_flags_for(Os::Linux, false));
    assert_eq!(open_flags(true, true), open_flags_for(Os::Darwin, true));
    assert_eq!(stat_layout(false, true), stat_layout_for(Os::Linux, true));
    assert_eq!(stat_layout(true, false), stat_layout_for(Os::Darwin, false));
}

#[test]
fn wasi_stat_has_linux_x86_64s_offsets_and_its_own_nofollow() {
    let wasi = stat_layout_for(Os::Wasi, false);
    let linux = stat_layout_for(Os::Linux, false);
    assert_eq!(
        (wasi.size, wasi.mode, wasi.mode_bits, wasi.st_size, wasi.mtime),
        (linux.size, linux.mode, linux.mode_bits, linux.st_size, linux.mtime)
    );
    assert_eq!(wasi.no_follow, 0x1, "AT_SYMLINK_NOFOLLOW, not Linux's 0x100");
}

#[test]
fn wasi_dirent_and_its_type_numbers() {
    let layout = dirent_layout_for(Os::Wasi);
    assert_eq!(
        (layout.d_type, layout.d_name),
        (8, 9),
        "{{ ino_t d_ino; u8 d_type; char d_name[] }}"
    );
    assert_eq!(
        dirent_types(Os::Wasi),
        DirentTypes { unknown: 0, link: 7, dir: 3, reg: 4 },
        "wasi-libc's DT_LNK, DT_DIR and DT_REG"
    );
    assert_eq!(dirent_types(Os::Linux), dirent_types(Os::Darwin));
    assert_eq!(dirent_types(Os::Linux), DirentTypes { unknown: 0, link: 10, dir: 4, reg: 8 });
    assert_eq!(enametoolong_for(Os::Wasi), enametoolong_for(Os::Linux), "WASI errno is translated");
}

#[test]
fn every_wasi_errno_is_translated_to_a_distinct_linux_one() {
    assert_eq!(WASI_ERRNO_TO_LINUX.len(), 76, "wasi-libc's __errno_values.h defines 76");
    let mut wasi: Vec<i64> = WASI_ERRNO_TO_LINUX.iter().map(|r| r.1).collect();
    wasi.sort_unstable();
    wasi.dedup();
    assert_eq!(wasi.len(), 76, "a WASI number appears once");
    assert!(WASI_ERRNO_TO_LINUX.iter().all(|r| r.2 > 0), "nothing maps to 'no error'");
    // Two WASI names may share a Linux number only where Linux has one errno for
    // both: ENOTCAPABLE has no Linux errno and is EPERM, so EPERM and ENOTCAPABLE
    // meet there, and nothing else does.
    let mut linux: Vec<(i64, &str)> = WASI_ERRNO_TO_LINUX.iter().map(|r| (r.2, r.0)).collect();
    linux.sort_unstable();
    let shared: Vec<&str> = linux.windows(2).filter(|w| w[0].0 == w[1].0).map(|w| w[1].1).collect();
    assert_eq!(shared.len(), 1, "{shared:?}");
}

#[test]
fn the_errnos_a_program_compares_against_mean_the_same_thing_on_wasi() {
    for (name, wasi, linux) in [
        ("ENOENT", 44, 2),
        ("EINVAL", 28, 22),
        ("EEXIST", 20, 17),
        ("EACCES", 2, 13),
        ("ENAMETOOLONG", 37, 36),
        ("ENOTDIR", 54, 20),
        ("EBADF", 8, 9),
    ] {
        assert_eq!(linux_errno_from_wasi(wasi), linux, "{name}");
    }
    assert_eq!(linux_errno_from_wasi(0), 0, "no error stays no error");
    assert_eq!(linux_errno_from_wasi(9999), 9999, "an unknown number is not invented");
    assert_eq!(enametoolong_for(Os::Wasi), linux_errno_from_wasi(37));
}

#[test]
fn only_wasi_refuses_anything_and_what_it_refuses_is_a_gap() {
    use crate::{Builtin, Gap, unsupported_on_target, wasi_gap};
    let program = super::tests::lower_src(
        "edition 4;\n\
         fn worker(x: int) -> [] int { return x * 2; }\n\
         fn main(world: World) -> [conc] int {\n\
             let Split { io, ffi, fs, heap, args, net } = split(world);\n\
             release(io); release(ffi); release(fs); release(heap); release(args); release(net);\n\
             let w = worker;\n\
             let h = spawn(21, w);\n\
             return join(h) - 42;\n\
         }\n",
    )
    .expect("a program that spawns");
    for os in [Os::Linux, Os::Darwin] {
        assert!(unsupported_on_target(&program, os, "host").is_empty(), "{os:?}");
    }
    let refusals = unsupported_on_target(&program, Os::Wasi, "wasm32-wasip1");
    assert_eq!(refusals.len(), 1, "one function, one family, one sentence: {refusals:?}");
    assert_eq!(refusals[0].rule, cancho_syntax::Rule::UnsupportedOnTarget);
    assert!(refusals[0].message.contains("`main` uses `spawn`"), "{}", refusals[0].message);

    // The families, by builtin, and the two lines that must not move.
    assert_eq!(wasi_gap(Builtin::Spawn), Some(Gap::Threads));
    assert_eq!(wasi_gap(Builtin::TcpConnect), Some(Gap::Sockets));
    assert_eq!(wasi_gap(Builtin::PollerWait), Some(Gap::Polling));
    assert_eq!(wasi_gap(Builtin::SignalsWatch), Some(Gap::Signals));
    assert_eq!(wasi_gap(Builtin::ExecSpawn), Some(Gap::Processes));
    // Found by running the builtins no accept fixture reaches: `file_lock` is an
    // undefined `flock` at link time, and `dir_mode` / `dir_own_mode` *build and run*
    // and answer 0 for every file, because WASI's stat has no permission bits.
    assert_eq!(wasi_gap(Builtin::FileLock), Some(Gap::Locks));
    assert_eq!(wasi_gap(Builtin::DirMode), Some(Gap::Permissions));
    assert_eq!(wasi_gap(Builtin::DirOwnMode), Some(Gap::Permissions));
    assert_eq!(wasi_gap(Builtin::DirRenameNew), Some(Gap::NoReplaceRename));
    // The replacing rename is on WASI's side of the line; only the no-replace one is not.
    assert_eq!(wasi_gap(Builtin::DirRename), None);
    for supported in [Builtin::DirStat, Builtin::FileSync, Builtin::DirSync, Builtin::FsRead] {
        assert_eq!(wasi_gap(supported), None, "{}", supported.name());
    }
    for supported in [Builtin::PutChar, Builtin::GetChar, Builtin::ReadFile, Builtin::ClockMs] {
        assert_eq!(wasi_gap(supported), None, "{}", supported.name());
    }
    assert_eq!(
        Builtin::ALL.iter().filter(|b| wasi_gap(**b).is_some()).count(),
        69,
        "the refused set moved: update docs/wasm.md with it"
    );
}

#[test]
fn an_open_mode_is_the_flags_its_fopen_string_means() {
    // `wb`, `ab`, `wbx` and `r+b`, on a target where read-write is not 2: WASI's
    // `O_RDWR` is `O_RDONLY | O_WRONLY`, so a backend that wrote `2` would ask for
    // an access mode that does not exist there.
    let w = open_flags_for(Os::Wasi, false);
    assert_eq!(w.read_write, w.read_only | w.write_only, "O_RDWR on WASI");
    assert_eq!(OpenMode::Write.open_flags(&w), w.write_only | w.create | w.truncate);
    assert_eq!(OpenMode::Append.open_flags(&w), w.write_only | w.create | w.append);
    assert_eq!(OpenMode::New.open_flags(&w), w.write_only | w.create | w.exclusive);
    assert_eq!(OpenMode::ReadWrite.open_flags(&w), 0x1400_0000);
    assert_eq!(OpenMode::Read.open_flags(&w), 0x0400_0000, "a read-only open asks for rights");

    // Linux and Darwin: O_RDWR is 2, the others as ever, and `Read` stays zero.
    for (os, aarch64) in [(Os::Linux, false), (Os::Linux, true), (Os::Darwin, false)] {
        let f = open_flags_for(os, aarch64);
        assert_eq!(f.read_write, 2, "{os:?}");
        assert_eq!(OpenMode::ReadWrite.open_flags(&f), 2);
        assert_eq!(OpenMode::Read.open_flags(&f), 0);
        assert_eq!(OpenMode::Write.open_flags(&f), f.write_only | f.create | f.truncate);
    }
}
