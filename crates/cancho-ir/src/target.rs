//! What a target cannot do (`docs/wasm.md`).
//!
//! A program built for WebAssembly under WASI preview 1 has no threads, no
//! sockets, no signals and no processes. Without this pass, a program that
//! reaches one **builds** and then dies: a thread test traps at run time, a
//! socket call is an `undefined symbol` from the linker, and neither says where
//! in the program the cause is. This walks every function the program reaches
//! (`Program::funcs` *is* the reachable set, `docs/authority.md`) and refuses,
//! at the function, with rule `unsupported-on-target`, before any code is
//! generated.
//!
//! It answers per `(function, family)`, once, naming the first builtin it found,
//! because a function that opens five sockets has one thing to change and five
//! identical sentences teach a reader to skim.
//!
//! The walk is the same exhaustive `match` [`collect_extern_refs`] uses, for the
//! same reason: a future `Expr` variant with no arm here is a compile error,
//! rather than a call this pass silently misses.
//!
//! Foreign functions are not classified. An `extern fn` names a C symbol, and
//! whether that symbol exists on the target is the linker's to say; it is the
//! one remaining place a target gap can still be a link error.
//!
//! [`collect_extern_refs`]: crate::fold::collect_extern_refs

use cancho_syntax::{Diagnostic, Rule};

use crate::{Builtin, Callee, Expr, Os, Program, Stmt};

/// A kind of thing the target lacks. One sentence of why, because the reason
/// is what tells a reader whether it will ever change.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Gap {
    Threads,
    Sockets,
    Polling,
    Signals,
    Processes,
    Locks,
    Permissions,
    NoReplaceRename,
    /// `docs/tty.md` §9: no termios, and no third target is claimed.
    Termios,
}

impl Gap {
    fn noun(self) -> &'static str {
        match self {
            Gap::Threads => "threads",
            Gap::Sockets => "sockets",
            Gap::Polling => "the poller",
            Gap::Signals => "signals",
            Gap::Processes => "processes and pipes",
            Gap::Locks => "file locks",
            Gap::Permissions => "permission bits",
            Gap::NoReplaceRename => "no-replace renames",
            Gap::Termios => "serial ports",
        }
    }

    fn why(self) -> &'static str {
        match self {
            Gap::Threads => "WASI preview 1 has no threads (wasi-threads is experimental)",
            Gap::Sockets => "WASI preview 1 has no sockets (`wasi:sockets` arrives with preview 2)",
            Gap::Polling => {
                "WASI has no epoll or kqueue, and `poll_oneoff` is not wired to the poller yet"
            }
            Gap::Signals => "WASI has no signals",
            Gap::Processes => "WASI has no processes and no `pipe`",
            Gap::Locks => "WASI has no `flock`, so it is an undefined symbol at link time",
            Gap::Permissions => {
                "WASI's stat has no permission bits (wasi-libc's `st_mode` carries the file type \
                 and nothing else), so `dir_mode` would answer 0 for every file"
            }
            Gap::NoReplaceRename => {
                "WASI's `path_rename` replaces an existing destination and has no no-replace flag, \
                 so `dir_rename_new` could only look and then rename, the window it exists to close"
            }
            Gap::Termios => {
                "WASI has no termios: a serial port is a host device (`docs/tty.md` §9)"
            }
        }
    }
}

/// What a builtin needs that WASI preview 1 does not have, if anything.
///
/// Spelled out as a `match` with no wildcard, so a new builtin is a compile
/// error here and someone has to say which side of the line it is on. The
/// second arm is long on purpose: it is the list of things WASI *can* do.
pub fn wasi_gap(builtin: Builtin) -> Option<Gap> {
    use Builtin as B;
    match builtin {
        B::Spawn | B::Join | B::ForkClock | B::ForkHeap => Some(Gap::Threads),
        B::Connect
        | B::Bind
        | B::Listen
        | B::ListenerClose
        | B::ListenerNonblocking
        | B::TcpListen
        | B::TcpConnect
        | B::TcpConnectStart
        | B::TcpAccept
        | B::ConnAttach
        | B::ConnClose
        | B::ConnConnectStatus
        | B::ConnDetach
        | B::ConnNodelay
        | B::ConnPeer
        | B::ConnNonblocking
        | B::ConnRead
        | B::ConnWrite
        | B::UdpConnect
        | B::UdpBind
        | B::UdpRecvFrom
        | B::UdpSendTo
        | B::UdpRecv
        | B::UdpSend
        | B::UdpLocalPort
        | B::UdpPeer
        | B::UdpNonblocking
        | B::UdpClose
        | B::UdpDetach
        | B::UdpAttach => Some(Gap::Sockets),
        B::PollerNew
        | B::PollerClose
        | B::PollerAddChild
        | B::PollerAddConn
        | B::PollerAddListener
        | B::PollerAddPipe
        | B::PollerAddUdp
        | B::PollerAddSignals
        | B::PollerModify
        | B::PollerRemove
        | B::PollerWait => Some(Gap::Polling),
        B::SignalsWatch | B::SignalsPending | B::SignalsClose => Some(Gap::Signals),
        // `docs/tty.md` §9: no termios under WASI, and no third target is
        // claimed — a new spike, not a port of the design.
        B::TtyOpen
        | B::TtyConfigure
        | B::TtyRead
        | B::TtyWrite
        | B::TtyFlushInput
        | B::TtyClose
        | B::PollerAddTty => Some(Gap::Termios),
        B::FileLock => Some(Gap::Locks),
        B::DirMode | B::DirOwnMode => Some(Gap::Permissions),
        B::DirRenameNew => Some(Gap::NoReplaceRename),
        B::ExecSpawn
        | B::ExecSpawnIn
        | B::ChildEndClose
        | B::ChildKill
        | B::ChildWait
        | B::PipeClose
        | B::PipeNonblocking
        | B::PipeOpen
        | B::PipeRead
        | B::PipeWrite => Some(Gap::Processes),
        B::PutChar
        | B::Write
        | B::WriteErr
        | B::FlushOut
        | B::GetChar
        | B::Split
        | B::Narrow
        | B::CopyWithin
        | B::CopyInto
        | B::IndexOfByte
        | B::Release
        | B::WrappingAdd
        | B::WrappingSub
        | B::WrappingMul
        | B::ValueBarrier
        | B::HwAesGcm
        | B::AesEncryptBlock
        | B::GhashUpdate
        | B::ByteOf
        | B::FloatOf
        | B::Truncate
        | B::BitsOf
        | B::FloatOfBits
        | B::F32Of
        | B::FloatOf32
        | B::BitsOf32
        | B::F32OfBits
        | B::Sqrt32
        | B::F32OfInt
        | B::IntOfF32
        | B::IsNan
        | B::Sqrt
        | B::IntOf
        | B::FsRead
        | B::OpenRead
        | B::ReadFile
        | B::Close
        | B::OpenAppend
        | B::OpenWrite
        | B::OpenNew
        | B::OpenRw
        | B::FsRename
        | B::FsRemove
        | B::FileWrite
        | B::FilePwrite
        | B::FilePread
        | B::FileSync
        | B::FileTruncate
        | B::FileSize
        | B::FsWrite
        | B::Box
        | B::Unbox
        | B::Contents
        | B::BoxSlice
        | B::UnboxSlice
        | B::ArgCount
        | B::Arg
        | B::Len
        | B::Accept
        | B::ClockMs
        | B::ClockUnixMs
        | B::OpenDir
        | B::DirEnter
        | B::DirOpenRead
        | B::DirClose
        | B::DirOpenNew
        | B::DirOpenAppend
        | B::DirRename
        | B::DirRemove
        | B::DirSync
        | B::DirList
        | B::DirNext
        | B::DirListClose
        | B::DirStat
        | B::NullPtr
        | B::Trap => None,
    }
}

/// Every function the program reaches that needs something `os` lacks.
///
/// Only WASI lacks anything today; Linux and Darwin answer an empty list.
pub fn unsupported_on_target(program: &Program, os: Os, target: &str) -> Vec<Diagnostic> {
    if os != Os::Wasi {
        return Vec::new();
    }
    let mut out = Vec::new();
    for func in &program.funcs {
        let mut found: Vec<(Gap, &'static str)> = Vec::new();
        body_gaps(&func.body, &mut found);
        let mut seen = std::collections::BTreeSet::new();
        for (gap, what) in found {
            if !seen.insert(gap) {
                continue;
            }
            let name = if func.module.is_empty() {
                func.name.clone()
            } else {
                format!("{}.{}", func.module, func.name)
            };
            out.push(Diagnostic::new(
                Rule::UnsupportedOnTarget,
                format!(
                    "`{name}` uses `{what}`, and {} do not exist on `{target}`: {} (docs/wasm.md)",
                    gap.noun(),
                    gap.why()
                ),
                func.span,
            ));
        }
    }
    out
}

fn body_gaps(body: &[Stmt], out: &mut Vec<(Gap, &'static str)>) {
    for stmt in body {
        match stmt {
            Stmt::Store { value, .. } | Stmt::Eval(value) | Stmt::Return(value) => {
                expr_gaps(value, out)
            }
            Stmt::If { cond, then_body, else_body } => {
                expr_gaps(cond, out);
                body_gaps(then_body, out);
                body_gaps(else_body, out);
            }
            Stmt::While { cond, body } => {
                expr_gaps(cond, out);
                body_gaps(body, out);
            }
            Stmt::Borrow { body, .. } | Stmt::Region { body, .. } => body_gaps(body, out),
            Stmt::Match { scrutinee, arms, .. } => {
                expr_gaps(scrutinee, out);
                for arm in arms {
                    body_gaps(&arm.body, out);
                }
            }
        }
    }
}

fn expr_gaps(e: &Expr, out: &mut Vec<(Gap, &'static str)>) {
    match e {
        Expr::Call { callee, args } => {
            if let Callee::Builtin(builtin) = callee
                && let Some(gap) = wasi_gap(*builtin)
            {
                out.push((gap, builtin.name()));
            }
            for a in args {
                expr_gaps(a, out);
            }
        }
        Expr::Int(_)
        | Expr::Bool(_)
        | Expr::Float(_)
        | Expr::F32(_)
        | Expr::Load(_)
        | Expr::Bytes(_)
        | Expr::Static(_) => {}
        Expr::FieldRef { base, .. }
        | Expr::FieldAddr { base, .. }
        | Expr::Field { base, .. }
        | Expr::TupleField { base, .. }
        | Expr::TupleFieldRef { base, .. }
        | Expr::TupleFieldAddr { base, .. }
        | Expr::Len(base)
        | Expr::Neg(base)
        | Expr::Not(base)
        | Expr::BitNot(base)
        | Expr::Deref { value: base, .. }
        | Expr::Boxed { value: base, .. }
        | Expr::Unboxed { value: base, .. }
        | Expr::Contents { value: base, .. }
        | Expr::UnboxedSlice { value: base }
        | Expr::Alloc { value: base, .. } => expr_gaps(base, out),
        Expr::Index { base, index, .. } => {
            expr_gaps(base, out);
            expr_gaps(index, out);
        }
        Expr::Subslice { base, start, end, .. } => {
            expr_gaps(base, out);
            expr_gaps(start, out);
            expr_gaps(end, out);
        }
        Expr::AllocSlice { count, fill, .. } | Expr::BoxedSlice { count, fill, .. } => {
            expr_gaps(count, out);
            expr_gaps(fill, out);
        }
        Expr::Struct { fields, .. }
        | Expr::Tuple { parts: fields }
        | Expr::Enum { payload: fields, .. } => {
            for f in fields {
                expr_gaps(f, out);
            }
        }
        Expr::Bin { lhs, rhs, .. } => {
            expr_gaps(lhs, out);
            expr_gaps(rhs, out);
        }
        // The forms that are not `Callee::Builtin` calls but are the same
        // builtins: each has its own `Expr` variant.
        Expr::ExecSpawn { args, .. } => {
            out.push((Gap::Processes, "exec_spawn"));
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::Connect { args, .. } => {
            out.push((Gap::Sockets, "connect"));
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::Bind { args, .. } => {
            out.push((Gap::Sockets, "bind"));
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::TcpListen { args, .. } => {
            out.push((Gap::Sockets, "tcp_listen"));
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::TcpConnect { args, .. } => {
            out.push((Gap::Sockets, "tcp_connect"));
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::FileOp { args, .. }
        | Expr::OpenFile { args, .. }
        | Expr::TtyOpen { args, .. }
        | Expr::PathOp { args, .. } => {
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::FnValue(_) => {}
        Expr::CallIndirect { target, args, .. } => {
            expr_gaps(target, out);
            args.iter().for_each(|a| expr_gaps(a, out));
        }
        Expr::Joined { handle, .. } => {
            out.push((Gap::Threads, "join"));
            expr_gaps(handle, out);
        }
    }
}
