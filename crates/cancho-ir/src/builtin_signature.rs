//! `Builtin::signature`, out of `builtin.rs` so that file stays under
//! the 2,000-line ceiling (`crates/cancho/tests/files.rs`).

use crate::*;

impl Builtin {
    /// Parameter types and return type, in terms of the prelude's ids.
    ///
    /// `prelude` is `[World, Io, Split]` — the ids `collect_types` handed
    /// out, which are fixed because the prelude is collected first.
    pub fn signature(self, prelude: &[DefId]) -> (Vec<Type>, Type) {
        let named = |i: usize| Type::Named(prelude[i], Vec::new());
        match self {
            Builtin::PutChar => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_IO)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            // The same borrowed `Io`, and a shared slice of the bytes.
            // Shared rather than unique because writing reads them, and
            // `strings.md` §4's coercion lets a caller hand over a
            // unique one anyway.
            //
            // `write_err` is the same signature on the other stream, so
            // it shares this arm rather than repeating it: a difference
            // between them here would be a difference nothing asked for
            // (`docs/standard-error.md` §3).
            Builtin::Write | Builtin::WriteErr => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_IO)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            // The borrowed `Io` and nothing else; the answer is the write
            // side's `Done`, so the errno survives (`docs/checked-output.md`).
            Builtin::FlushOut => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_IO)),
                }],
                named(PRELUDE_DONE),
            ),
            // The mirror: the same borrowed `Io`, no character to take.
            Builtin::GetChar => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_IO)),
                }],
                Type::Int,
            ),
            // Checked at the call site (`docs/editions.md` §7): the return
            // type depends on the caller's edition, and a fixed signature
            // cannot say that.
            Builtin::Split => (Vec::new(), Type::Unit),
            Builtin::WrappingAdd | Builtin::WrappingSub | Builtin::WrappingMul => {
                (vec![Type::Int, Type::Int], Type::Int)
            }
            Builtin::ValueBarrier => (vec![Type::Int], Type::Int),
            // `docs/crypto-builtins.md` §3.
            Builtin::HwAesGcm => (Vec::new(), Type::Bool),
            Builtin::AesEncryptBlock => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Int,
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(2),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::GhashUpdate => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(2),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::Len => (Vec::new(), Type::Int),
            // Both are checked at the call site: the prefix in the
            // capability's type is what decides the row, and a fixed
            // signature cannot say that.
            Builtin::FsRead | Builtin::FsWrite => (Vec::new(), Type::Unit),
            // Checked at the call site, exactly as `fs_read` is: the prefix
            // is in the capability's type (`docs/file-handles.md` §2.1).
            Builtin::OpenRead
            | Builtin::OpenAppend
            | Builtin::OpenWrite
            | Builtin::OpenNew
            | Builtin::OpenRw
            | Builtin::FsRename
            | Builtin::FsRemove
            | Builtin::OpenDir => (Vec::new(), Type::Unit),
            // The handle is borrowed uniquely because the read moves the
            // descriptor's offset, and the buffer uniquely because the read
            // writes into it -- the same pair `fs_read` takes, with the
            // capability replaced by the handle it was spent on.
            Builtin::ReadFile => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_READ),
            ),
            // `docs/file-writes.md` section 4. The handle is borrowed
            // uniquely (a write moves the cursor and a sync is an
            // operation on it), the source buffer shared and the
            // destination buffer unique, the pair `conn_write` and
            // `file_read` take.
            Builtin::FileWrite => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DONE),
            ),
            Builtin::FilePwrite => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Int,
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DONE),
            ),
            Builtin::FilePread => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Int,
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_READ),
            ),
            Builtin::FileSync | Builtin::FileSize | Builtin::FileLock => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_FILE)),
                }],
                named(PRELUDE_DONE),
            ),
            Builtin::FileTruncate => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_FILE)),
                    },
                    Type::Int,
                ],
                named(PRELUDE_DONE),
            ),
            // By value: `close` ends the handle, which is what `res` means.
            Builtin::Close => (vec![named(PRELUDE_FILE)], Type::Int),
            // All three depend on the type being boxed, which a fixed
            // signature has no parameter to name (`docs/heap.md` §3).
            Builtin::Box
            | Builtin::Unbox
            | Builtin::Contents
            | Builtin::BoxSlice
            | Builtin::UnboxSlice => (Vec::new(), Type::Unit),
            // `docs/arguments.md` §3. Written out rather than checked at
            // the call site, because neither depends on a type the caller
            // chose: an argument is always `&static [byte]`.
            Builtin::ArgCount => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_ARGS)),
                }],
                Type::Int,
            ),
            Builtin::Arg => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_ARGS)),
                    },
                    Type::Int,
                ],
                Type::Ref {
                    unique: false,
                    region: Region::Static,
                    inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                },
            ),
            Builtin::ByteOf => (vec![Type::Int], Type::Byte),
            Builtin::IntOf => (vec![Type::Byte], Type::Int),
            Builtin::FloatOf => (vec![Type::Int], Type::Float),
            Builtin::Truncate => (vec![Type::Float], Type::Int),
            Builtin::IsNan => (vec![Type::Float], Type::Bool),
            // Float in, float out, and nothing else: no capability, because
            // it reaches no library (`docs/float-math.md` §3).
            Builtin::Sqrt => (vec![Type::Float], Type::Float),
            Builtin::BitsOf => (vec![Type::Float], Type::Int),
            Builtin::FloatOfBits => (vec![Type::Int], Type::Float),
            Builtin::F32Of => (vec![Type::Float], Type::F32),
            Builtin::FloatOf32 => (vec![Type::F32], Type::Float),
            Builtin::BitsOf32 => (vec![Type::F32], Type::Int),
            Builtin::F32OfBits => (vec![Type::Int], Type::F32),
            Builtin::Sqrt32 => (vec![Type::F32], Type::F32),
            Builtin::F32OfInt => (vec![Type::Int], Type::F32),
            Builtin::IntOfF32 => (vec![Type::F32], Type::Int),
            // Both are checked at the call site rather than here, because a
            // fixed signature cannot say what they need. `release` ends any
            // capability, and there is more than one kind; `narrow` has an
            // argument *and* a result that depend on the literal written at
            // the call.
            Builtin::Release | Builtin::Narrow => (Vec::new(), Type::Unit),
            // Checked at the call site, like `box`: the argument must be a
            // uniquely borrowed `Heap`.
            Builtin::ForkHeap => (Vec::new(), Type::Unit),
            // Checked at the call site, exactly as `fs_read` is: the bound
            // is in the capability's type, and a fixed signature cannot
            // say that (`docs/net.md` §4.1).
            Builtin::Connect => (Vec::new(), Type::Unit),
            // Same reason, for the inbound half's bound (`docs/listen.md`
            // §6.1).
            Builtin::Bind => (Vec::new(), Type::Unit),
            // Neither takes a capability -- the fd already proves the
            // authority `bind` checked -- so both are ordinary fixed
            // signatures (`docs/listen.md` §6).
            Builtin::Listen => (vec![Type::Int, Type::Int], Type::Int),
            Builtin::Accept => (vec![Type::Int], Type::Int),
            // Checked at the call site, like `bind`: the port is spent
            // against the bound in the capability's type.
            Builtin::TcpListen | Builtin::TcpConnect | Builtin::TcpConnectStart => {
                (Vec::new(), Type::Unit)
            }
            Builtin::TcpAccept => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_LISTENER)),
                }],
                named(PRELUDE_ACCEPTED),
            ),
            // The handle is borrowed uniquely for a read (it moves the
            // stream) and the buffer is written into.
            Builtin::ConnRead => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_CONN)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_RECEIVED),
            ),
            // The handle is unique -- a write moves the stream -- and the
            // buffer is only read, so a program can send from the same
            // bytes it is parsing.
            Builtin::ConnWrite => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_CONN)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_SENT),
            ),
            Builtin::PollerNew => (Vec::new(), named(PRELUDE_POLLING)),
            Builtin::PollerAddListener => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_LISTENER)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::PollerAddConn | Builtin::PollerModify => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_CONN)),
                    },
                    Type::Int,
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::PollerRemove => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_CONN)),
                    },
                ],
                Type::Int,
            ),
            // The events land in a slice of ints: `(token, events)` pairs.
            Builtin::PollerWait => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Int))),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::PollerClose => (vec![named(PRELUDE_POLLER)], Type::Int),
            // Checked at the call site: the set is in the capability's type.
            Builtin::SignalsWatch => (Vec::new(), Type::Unit),
            Builtin::SignalsPending => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_SIGNAL_WATCH)),
                }],
                Type::Int,
            ),
            Builtin::PollerAddSignals => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_SIGNAL_WATCH)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::SignalsClose => (vec![named(PRELUDE_SIGNAL_WATCH)], Type::Int),
            Builtin::DirEnter | Builtin::DirOpenRead => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                if self == Builtin::DirEnter {
                    named(PRELUDE_DIR_OPENED)
                } else {
                    named(PRELUDE_OPENED)
                },
            ),
            Builtin::DirClose => (vec![named(PRELUDE_DIR)], Type::Int),
            Builtin::DirOpenNew | Builtin::DirOpenAppend | Builtin::DirRemove => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                if self == Builtin::DirRemove {
                    named(PRELUDE_DONE)
                } else {
                    named(PRELUDE_OPENED)
                },
            ),
            Builtin::DirRename | Builtin::DirRenameNew => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(2),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DONE),
            ),
            Builtin::DirSync => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_DIR)),
                }],
                named(PRELUDE_DONE),
            ),
            // `docs/directory-listing.md` §3.1. The directory is shared, as
            // every step beneath it is; the listing is unique, because a
            // step moves it, and so is the buffer the name is copied into.
            Builtin::DirList => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_DIR)),
                }],
                named(PRELUDE_LISTING),
            ),
            Builtin::DirNext => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR_LIST)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_LISTED),
            ),
            Builtin::DirListClose => (vec![named(PRELUDE_DIR_LIST)], Type::Int),
            // §3.2: `dir_enter`'s shape, answering a status.
            Builtin::DirStat => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DIR_STAT),
            ),
            // §3.5: `dir_stat`'s shape, answering the bits as `Done`.
            Builtin::DirMode => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_DIR)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DONE),
            ),
            Builtin::DirOwnMode => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_DIR)),
                }],
                named(PRELUDE_DONE),
            ),
            Builtin::ConnDetach => (vec![named(PRELUDE_CONN)], Type::Int),
            Builtin::ConnAttach => (vec![Type::Int], named(PRELUDE_ATTACHED)),
            Builtin::ClockMs | Builtin::ClockUnixMs => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_CLOCK)),
                }],
                Type::Int,
            ),
            Builtin::CopyWithin => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Int,
                    Type::Int,
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::CopyInto => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::IndexOfByte => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Byte,
                ],
                Type::Int,
            ),
            Builtin::ForkClock => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_CLOCK)),
                }],
                named(PRELUDE_CLOCK),
            ),
            // `docs/conn-peer.md` §3: the connection is only read (`getpeername` moves
            // nothing), the buffer is written into.
            Builtin::ConnPeer => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_CONN)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::ConnNonblocking | Builtin::ConnNodelay | Builtin::ConnConnectStatus => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_CONN)),
                }],
                Type::Int,
            ),
            Builtin::ListenerNonblocking => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_LISTENER)),
                }],
                Type::Int,
            ),
            // By value: `close` ends the handle.
            Builtin::ConnClose => (vec![named(PRELUDE_CONN)], Type::Int),
            // `docs/processes.md` §3.2.
            Builtin::PipeOpen => (Vec::new(), named(PRELUDE_PIPED)),
            // Checked at the call site: the prefix is in the capability's type.
            Builtin::ExecSpawn | Builtin::ExecSpawnIn => (Vec::new(), Type::Unit),
            // By value: waiting ends the child.
            Builtin::ChildWait => (vec![named(PRELUDE_CHILD)], named(PRELUDE_EXITED)),
            Builtin::ChildKill => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_CHILD)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::PipeRead | Builtin::PipeWrite => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_PIPE)),
                    },
                    Type::Ref {
                        unique: self == Builtin::PipeRead,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(if self == Builtin::PipeRead { PRELUDE_RECEIVED } else { PRELUDE_SENT }),
            ),
            Builtin::PipeNonblocking => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_PIPE)),
                }],
                Type::Int,
            ),
            Builtin::PipeClose => (vec![named(PRELUDE_PIPE)], Type::Int),
            Builtin::ChildEndClose => (vec![named(PRELUDE_CHILD_END)], Type::Int),
            Builtin::PollerAddPipe | Builtin::PollerAddChild => {
                let handle =
                    if self == Builtin::PollerAddPipe { PRELUDE_PIPE } else { PRELUDE_CHILD };
                let mut params = vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(handle)),
                    },
                    Type::Int,
                ];
                if self == Builtin::PollerAddPipe {
                    params.push(Type::Int);
                }
                (params, Type::Int)
            }
            Builtin::ListenerClose => (vec![named(PRELUDE_LISTENER)], Type::Int),
            // `docs/udp.md` §3. `udp_connect` is checked at the call site, like
            // `tcp_connect`: the bound is in the capability's type.
            Builtin::UdpConnect | Builtin::UdpBind => (Vec::new(), Type::Unit),
            // `docs/tty.md` §3, edition 8: the capability, borrowed and
            // shared, and the path; what opens answers is `TtyOpened`.
            Builtin::TtyOpen => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_TTY)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_TTY_OPENED),
            ),
            // The port handle, borrowed unique for configure (it is
            // mutated), plain for the rest, as `Conn`'s verbs split.
            Builtin::TtyConfigure => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_PORT)),
                    },
                    Type::Int,
                ],
                Type::Int,
            ),
            Builtin::TtyRead => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_PORT)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::TtyWrite => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_PORT)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::TtyFlushInput => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_PORT)),
                }],
                Type::Int,
            ),
            Builtin::TtyClose => (vec![named(PRELUDE_PORT)], Type::Int),
            Builtin::PollerAddTty => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_PORT)),
                    },
                    Type::Int,
                    Type::Int,
                ],
                Type::Int,
            ),
            // A receive that also writes the sender's ticket into the first cell of an `int`
            // slice (`docs/udp.md` §4).
            Builtin::UdpRecvFrom => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(2),
                        inner: Box::new(Type::Slice(Box::new(Type::Int))),
                    },
                ],
                named(PRELUDE_DATAGRAM),
            ),
            Builtin::UdpSendTo => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                    Type::Int,
                ],
                named(PRELUDE_SENT),
            ),
            // The handle is unique -- a receive moves the socket -- and the
            // buffer is written into.
            Builtin::UdpRecv => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_DATAGRAM),
            ),
            Builtin::UdpSend => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                named(PRELUDE_SENT),
            ),
            Builtin::UdpLocalPort => (
                vec![Type::Ref {
                    unique: false,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_UDP)),
                }],
                Type::Int,
            ),
            Builtin::UdpPeer => (
                vec![
                    Type::Ref {
                        unique: false,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Int,
                    Type::Ref {
                        unique: true,
                        region: Region::Param(1),
                        inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                    },
                ],
                Type::Int,
            ),
            Builtin::UdpNonblocking => (
                vec![Type::Ref {
                    unique: true,
                    region: Region::Param(0),
                    inner: Box::new(named(PRELUDE_UDP)),
                }],
                Type::Int,
            ),
            Builtin::UdpClose => (vec![named(PRELUDE_UDP)], Type::Int),
            Builtin::UdpDetach => (vec![named(PRELUDE_UDP)], Type::Int),
            Builtin::UdpAttach => (vec![Type::Int], named(PRELUDE_UDP_OPENED)),
            Builtin::PollerAddUdp => (
                vec![
                    Type::Ref {
                        unique: true,
                        region: Region::Param(0),
                        inner: Box::new(named(PRELUDE_POLLER)),
                    },
                    Type::Ref {
                        unique: false,
                        region: Region::Param(1),
                        inner: Box::new(named(PRELUDE_UDP)),
                    },
                    Type::Int,
                    Type::Int,
                ],
                Type::Int,
            ),
            // No capability, no data in, one opaque handle out
            // (`docs/opaque-pointers.md` §3) -- a fixed signature like
            // `sqrt`'s, not a call-site check like `len`'s.
            Builtin::NullPtr => (Vec::new(), Type::CPtr),
            // Checked at the call site, like `len`: `T` and `R` come
            // from `payload`'s and `body`'s own types
            // (`docs/threads.md` §2).
            Builtin::Spawn => (Vec::new(), Type::Unit),
            Builtin::Join => (Vec::new(), Type::Unit),
            // No arguments, no capability, and a fixed `int` result like
            // `byte_of`'s -- the value is never actually produced, since
            // the call never returns, but the type checker needs one to
            // check the call site the ordinary way.
            Builtin::Trap => (Vec::new(), Type::Int),
        }
    }
}
