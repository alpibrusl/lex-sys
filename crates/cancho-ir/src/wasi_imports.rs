//! What a program's effect row licenses a WASI module to import (`docs/wasm.md`, W2b).
//!
//! A wasm module's import section is what the *runtime* is asked to grant it, and
//! it is enforced there. The row is what the *checker* says the program does. This
//! table is the bridge: for each effect label, the WASI preview 1 functions a module
//! built from a program that performs it may import.
//!
//! Two sets per label, because a label is coarser than a builtin. `dir_write`
//! covers creating, appending, renaming, removing and syncing; a program with that
//! label may use only one of them, and imports only that one's functions.
//!
//! * **`allowed`**: every function any builtin under the label can bring in. A
//!   module importing something no label in its row allows has been granted a
//!   capability its row does not account for. That is the half that matters for
//!   authority, and the one a check must never let slip.
//! * **`required`**: what *every* builtin under the label imports, so the
//!   intersection. A row is exact in both directions (`docs/linearity-and-effects.md`
//!   §7.3), which means a label in it is performed by something, so a module
//!   missing a required import is a module whose row claims more than it does.
//!
//! A program with an empty row has an **empty import section**: nothing is imported
//! unless a label licenses it. The one exception is [`EXIT`], `proc_exit`, which is
//! *allowed* for every program and *required* of none, because a program leaves through it
//! only if it can return a non-zero status. (Until W2c every program also imported
//! `args_get` and `args_sizes_get`, because the command line was fetched before `main`.)
//!
//! **Where the numbers come from.** They are measured, not read off libc: each row
//! says by what. The rows measured by a single tiny program per builtin and by the
//! accept fixtures are checked against *every accept fixture* by
//! `scripts/wasm_authority_check.py`, which needs the wasm toolchain and so is not in
//! CI. What CI does check, here, is that the table accounts for every label the
//! checker can produce, that `required` is inside `allowed`, and that no row names a
//! function WASI preview 1 does not have.

use crate::{WORLD_PLAIN_LABELS, WORLD_ROOT_LABELS};

/// `proc_exit`: how a command leaves with a non-zero status. Allowed for every program,
/// required of none: a program whose `main` cannot return anything but 0 never reaches
/// the call, and its module does not import it (a program that reads nothing and prints
/// nothing imports *nothing*).
///
/// Until W2c there were three of these. wasi-libc's `__main_void` fetched the command
/// line, with `args_get` and `args_sizes_get`, before every `main`, so a program that
/// released `args` and never read one still imported both and the runtime would have
/// granted it the command line. The module's own `_start` (`wasi_entry`) fetches it only
/// for a program that reads it, which is the `args` label, so those two are now that
/// label's, like any other.
pub const EXIT: &[&str] = &["proc_exit"];

/// One label's row.
pub struct LabelImports {
    pub label: &'static str,
    pub required: &'static [&'static str],
    pub allowed: &'static [&'static str],
}

/// The labels a WASI target supports. Sorted, so a lookup and a diff are stable.
pub const LABEL_IMPORTS: &[LabelImports] = &[
    // Console: `scripts/wasm_console_check.py`, one program per effect, the set
    // required to be exact.
    LabelImports { label: "io_read", required: &["fd_read"], allowed: &["fd_read"] },
    LabelImports { label: "io_write", required: &["fd_write"], allowed: &["fd_write"] },
    LabelImports { label: "err_write", required: &["fd_write"], allowed: &["fd_write"] },
    // The heap grows linear memory with `memory.grow`, an instruction, not an import.
    LabelImports { label: "heap", required: &[], allowed: &[] },
    // `arg_count` and `arg` read what the module's own `_start` fetched, with these two
    // calls and only for a program that reads them (`wasm_console_check.py`, `args`).
    LabelImports {
        label: "args",
        required: &["args_get", "args_sizes_get"],
        allowed: &["args_get", "args_sizes_get"],
    },
    // `clock_ms` and `clock_unix_ms`, one probe.
    LabelImports { label: "clock", required: &["clock_time_get"], allowed: &["clock_time_get"] },
    // `fs_read(p)`: `fs_read`, `open_read` and `open_dir` all open a path, which costs
    // `path_open`, `fd_close`, `fd_fdstat_get` and the two calls wasi-libc makes to find
    // which preopened directory a path is under (`fd_prestat_get`, `fd_prestat_dir_name`),
    // so all five are required; reading a whole file adds `fd_read`.
    LabelImports {
        label: "fs_read",
        required: &[
            "fd_close",
            "fd_fdstat_get",
            "fd_prestat_dir_name",
            "fd_prestat_get",
            "path_open",
        ],
        allowed: &[
            "fd_close",
            "fd_fdstat_get",
            "fd_prestat_dir_name",
            "fd_prestat_get",
            "fd_read",
            "path_open",
        ],
    },
    // `fs_write(p)`: `fs_remove` and `fs_rename` import only the preopen discovery and
    // their one call (no `path_open`), so that discovery is all that is required;
    // `fs_write` and the four write-mode openers add the open group, `fs_write` adds
    // `fd_write`.
    LabelImports {
        label: "fs_write",
        required: &["fd_prestat_dir_name", "fd_prestat_get"],
        allowed: &[
            "fd_close",
            "fd_fdstat_get",
            "fd_prestat_dir_name",
            "fd_prestat_get",
            "fd_write",
            "path_open",
            "path_rename",
            "path_unlink_file",
        ],
    },
    // A handle's reads: `file_read` is `fd_read`, `file_pread` is `fd_pread`,
    // `file_size` is `fd_seek` (measured: `scripts/wasm_file_check.py --imports`).
    // No single one is common to all three, so nothing is required.
    LabelImports {
        label: "file_read",
        required: &[],
        allowed: &["fd_pread", "fd_read", "fd_seek"],
    },
    // A handle's writes: `file_write`, `file_pwrite`, `file_sync`, `file_truncate`
    // (`file_lock` is refused on WASI: no `flock`).
    LabelImports {
        label: "file_write",
        required: &[],
        allowed: &["fd_filestat_set_size", "fd_pwrite", "fd_sync", "fd_write"],
    },
    // Beneath a directory handle, reading: `dir_enter`, `dir_open_read` (`path_open`),
    // `dir_list` and `dir_next` (`fd_readdir`), `dir_stat` (`path_filestat_get`).
    // (`dir_mode` and `dir_own_mode` are refused on WASI: no permission bits.)
    // Measured on `directory_handles` and `directory_listing`.
    LabelImports {
        label: "dir_read",
        required: &[],
        allowed: &[
            "fd_close",
            "fd_fdstat_get",
            "fd_prestat_dir_name",
            "fd_prestat_get",
            "fd_readdir",
            "path_filestat_get",
            "path_open",
        ],
    },
    // Beneath a directory handle, writing: `dir_open_new` and `dir_open_append`
    // (`path_open`), `dir_rename`, `dir_remove` (`path_unlink_file` for a file,
    // `path_remove_directory` for a directory), `dir_sync` (`fd_sync`). Measured on the
    // driver `directory_writes.rs` runs, `scripts/wasm_file_check.py`.
    LabelImports {
        label: "dir_write",
        required: &[],
        allowed: &[
            "fd_close",
            "fd_fdstat_get",
            "fd_prestat_dir_name",
            "fd_prestat_get",
            "fd_sync",
            "path_open",
            "path_remove_directory",
            "path_rename",
            "path_unlink_file",
        ],
    },
];

/// The labels a WASI target cannot do at all: the families `wasi_gap` refuses
/// (`target.rs`), so a program carrying one never reaches a module.
pub const REFUSED_LABELS: &[&str] = &[
    "conc",
    "conn_accept",
    "conn_read",
    "conn_write",
    "udp_recv",
    "udp_send",
    "poll",
    "signals",
    "signals_read",
    "child_signal",
    "exec",
    "pipe_read",
    "pipe_write",
    "net_out",
    "net_in",
    // `docs/tty.md` §9: WASI has no termios, so every tty label is
    // refused — a serial port is a host device.
    "tty_open",
    "tty_read",
    "tty_write",
];

/// A label whose imports cannot be bounded: a foreign function names a C symbol,
/// and which WASI functions that symbol reaches is the library's, not the row's.
pub const UNBOUNDED_LABELS: &[&str] = &["ffi"];

/// What a row licenses a module to import.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WasiImports {
    /// What the row's labels require. Sorted, deduplicated.
    pub required: Vec<&'static str>,
    /// What the row's labels allow, and [`EXIT`]. Sorted, deduplicated.
    pub allowed: Vec<&'static str>,
    /// Labels in the row a WASI target cannot do; such a program is refused.
    pub refused: Vec<String>,
    /// Labels whose imports the row cannot bound (`ffi`).
    pub unbounded: Vec<String>,
    /// Labels the table does not know at all. The completeness test keeps this empty.
    pub unknown: Vec<String>,
}

/// The row for one label, if the target supports it.
pub fn label_imports(label: &str) -> Option<&'static LabelImports> {
    LABEL_IMPORTS.iter().find(|row| row.label == label)
}

/// What the labels of a row license a WASI module to import.
pub fn wasi_imports<'a>(labels: impl IntoIterator<Item = &'a str>) -> WasiImports {
    let mut out = WasiImports { allowed: EXIT.to_vec(), ..WasiImports::default() };
    for label in labels {
        if let Some(row) = label_imports(label) {
            out.required.extend(row.required);
            out.allowed.extend(row.allowed);
        } else if REFUSED_LABELS.contains(&label) {
            out.refused.push(label.to_owned());
        } else if UNBOUNDED_LABELS.contains(&label) {
            out.unbounded.push(label.to_owned());
        } else {
            out.unknown.push(label.to_owned());
        }
    }
    for set in [&mut out.required, &mut out.allowed] {
        set.sort_unstable();
        set.dedup();
    }
    for list in [&mut out.refused, &mut out.unbounded, &mut out.unknown] {
        list.sort();
        list.dedup();
    }
    out
}

/// Every label the checker can produce: the plain ones and the named ones a `World`
/// discharges, plus `conc`, which concurrency adds and no capability carries.
pub fn every_label() -> Vec<&'static str> {
    let mut all: Vec<&'static str> =
        WORLD_PLAIN_LABELS.iter().chain(WORLD_ROOT_LABELS).copied().collect();
    all.push("conc");
    all.sort_unstable();
    all.dedup();
    all
}

/// The functions of WASI preview 1 (`wasi_snapshot_preview1`), all 46 of them. The
/// table is checked against this, so a misspelt import is a test failure and not an
/// import no module will ever have.
pub const PREVIEW1_FUNCTIONS: &[&str] = &[
    "args_get",
    "args_sizes_get",
    "clock_res_get",
    "clock_time_get",
    "environ_get",
    "environ_sizes_get",
    "fd_advise",
    "fd_allocate",
    "fd_close",
    "fd_datasync",
    "fd_fdstat_get",
    "fd_fdstat_set_flags",
    "fd_fdstat_set_rights",
    "fd_filestat_get",
    "fd_filestat_set_size",
    "fd_filestat_set_times",
    "fd_pread",
    "fd_prestat_dir_name",
    "fd_prestat_get",
    "fd_pwrite",
    "fd_read",
    "fd_readdir",
    "fd_renumber",
    "fd_seek",
    "fd_sync",
    "fd_tell",
    "fd_write",
    "path_create_directory",
    "path_filestat_get",
    "path_filestat_set_times",
    "path_link",
    "path_open",
    "path_readlink",
    "path_remove_directory",
    "path_rename",
    "path_symlink",
    "path_unlink_file",
    "poll_oneoff",
    "proc_exit",
    "proc_raise",
    "random_get",
    "sched_yield",
    "sock_accept",
    "sock_recv",
    "sock_send",
    "sock_shutdown",
];

#[cfg(test)]
#[path = "tests/wasi_imports.rs"]
mod tests;
