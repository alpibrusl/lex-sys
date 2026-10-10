//! How a cancho value is held in machine types, how it crosses to C,
//! and the constants the emitted code and the runtime agree on.

use crate::*;

/// Does this parameter reach the foreign function, or stop at the checker?
///
/// A borrowed capability carries no data and stops here (§8.1). A byte
/// slice crosses as both its leaves — a pointer and a length — because
/// `docs/strings.md` §6 says C is handed the pair as two arguments.
pub(crate) fn crosses_to_c(ty: &Type) -> bool {
    match ty {
        Type::Ref { inner, .. } => {
            matches!(inner.as_ref(), Type::Slice(element) if **element == Type::Byte)
        }
        _ => true,
    }
}

/// The machine types a cancho type is held in, in field order.
///
/// A struct is **scalarised**: it is not a block of memory with a layout, it
/// is its leaf fields, each in its own register or stack argument. M1 has no
/// references and no recursive structs, so every value is a finite tree of
/// scalars and this always terminates.
///
/// That also means M1 makes no layout decisions at all. Deterministic layout
/// is a commitment (#1) and `docs/defined-behaviour.md` is where it gets
/// decided; choosing one here to make the backend easier would be choosing it
/// by accident.
///
/// `bool` is an `i8` because that is what Cranelift's `icmp` produces, so a
/// comparison *is* a `bool` with nothing to convert. M0 widened every
/// comparison to `i64` and called it an `int`; the type system now says what
/// was always true about the value.
pub(crate) fn leaves_into(
    ty: &Type,
    program: &Program,
    pointer: types::Type,
    out: &mut Vec<types::Type>,
) {
    match ty {
        Type::Int => out.push(types::I64),
        // `docs/opaque-pointers.md` §4: a handle rides in a register
        // exactly like an integer-shaped file descriptor already does
        // (`docs/reach.md` §3.1.1) -- one pointer-width leaf, same as a
        // reference's own leaf just below.
        Type::CPtr => out.push(pointer),
        // `docs/function-values.md` §4.2: `val`, one leaf, a pointer to
        // the target's compiled entry point -- the same treatment
        // `Type::CPtr` and a reference's own pointer leaf already get.
        Type::Fn(..) => out.push(pointer),
        // `docs/floating-point.md` §1: binary64, which is `F64` and
        // nothing else. A `float` is one leaf, like an `int`.
        Type::Float => out.push(types::F64),
        // `docs/f32.md` §2: binary32, which is `F32`. One leaf.
        Type::F32 => out.push(types::F32),
        // A byte and a bool are both one byte wide. That they share a
        // machine type is not an invitation to mix them: the checker keeps
        // them apart, and `byte` has no arithmetic to mix *with*.
        Type::Byte | Type::Bool => out.push(types::I8),
        // A generic type's members are written in terms of its parameters, so
        // they are substituted here rather than monomorphised: `Pair[int,
        // bool]` and `Pair[bool, int]` are two leaf layouts of one
        // declaration. Only *functions* are copied per instantiation.
        // `docs/heap.md` §3: a box at run time is a pointer and nothing
        // else -- no header, no refcount, no tag. That is why `contents` is
        // a load rather than a computation, and why §4 can let a type
        // contain itself through one: it is a single leaf however large what
        // it points at is.
        // `docs/file-handles.md`: a handle at run time is a descriptor and
        // nothing else -- one leaf, where the six capabilities are zero.
        // That is the whole difference between a `File` and an `Io`: one
        // is authority the type system tracks and the kernel has never
        // heard of, and the other is a number the kernel gave us.
        Type::Named(def, _)
            if matches!(
                def.0 as usize,
                cancho_ir::PRELUDE_FILE
                    | cancho_ir::PRELUDE_DIR
                    | cancho_ir::PRELUDE_DIR_LIST
                    | cancho_ir::PRELUDE_LISTENER
                    | cancho_ir::PRELUDE_CONN
                    | cancho_ir::PRELUDE_UDP
                    | cancho_ir::PRELUDE_POLLER
                    // `docs/tty.md` §3, edition 8: the port handle, a
                    // descriptor and nothing else — `Udp`'s shape.
                    | cancho_ir::PRELUDE_PORT
                    | cancho_ir::PRELUDE_SIGNAL_WATCH
                    // `docs/processes.md` §3.1: a pid, and two descriptors.
                    | cancho_ir::PRELUDE_CHILD
                    | cancho_ir::PRELUDE_PIPE
                    | cancho_ir::PRELUDE_CHILD_END
            ) =>
        {
            out.push(types::I64);
        }
        Type::Named(def, args) if def.0 as usize == cancho_ir::PRELUDE_BOX => {
            out.push(pointer);
            // `docs/boxed-slices.md` §2: the second shape. A box of an
            // *unsized* referent carries the length too, because nothing
            // else knows how many elements there are -- which is the same
            // pair `&r [T]` already is, and why `contents` needed no new
            // rule for it.
            if matches!(args.first(), Some(Type::Slice(_))) {
                out.push(types::I64);
            }
        }
        // `docs/threads.md` §2: a `Thread[T, R]` at run time is a real
        // `pthread_t` and nothing else -- one register-width leaf, the
        // same "no fields on purpose" shape `Box` already has, for the
        // same reason (a pattern that could name it would be a way to
        // end the `res` obligation without joining).
        Type::Named(def, _) if def.0 as usize == cancho_ir::PRELUDE_THREAD => {
            out.push(pointer);
        }
        // `docs/tuples.md` §5: a tuple's leaves are its components' leaves
        // in order -- a struct's layout with the names removed. No tag, no
        // new padding question, and nothing a `DefId` was needed for.
        Type::Tuple(parts) => {
            for part in parts {
                leaves_into(part, program, pointer, out);
            }
        }
        Type::Named(def, args) => match program.type_info(*def) {
            TypeInfo::Struct { fields, .. } => {
                for (_, field) in fields {
                    leaves_into(&field.substitute(args, &[]), program, pointer, out);
                }
            }
            // An enum is a tag followed by *every* variant's payload, each in
            // its own leaves. That is wasteful and deliberately so: overlaying
            // the payloads is a layout decision, and M1 owns no layout
            // decisions (#1, `docs/defined-behaviour.md` in M3). The tag is an
            // `i64` for the same reason — picking the narrowest integer that
            // fits would be choosing a representation.
            TypeInfo::Enum { variants, .. } => {
                out.push(types::I64);
                for (_, payload) in variants {
                    for ty in payload {
                        leaves_into(&ty.substitute(args, &[]), program, pointer, out);
                    }
                }
            }
        },
        // A reference is one pointer, whatever it points at -- except a
        // slice, which is a pointer *and* a length, because `[T]` is the one
        // referent whose size is not in its type. Regions are erased either
        // way: which block a reference came from is a fact the checker used
        // and the machine has no use for.
        Type::Ref { inner, .. } => {
            out.push(pointer);
            if matches!(inner.as_ref(), Type::Slice(_)) {
                out.push(types::I64);
            }
        }
        other => {
            unreachable!("`{other:?}` reached the backend; the checker should have refused it")
        }
    }
}

pub(crate) fn leaves(ty: &Type, program: &Program, pointer: types::Type) -> Vec<types::Type> {
    let mut out = Vec::new();
    leaves_into(ty, program, pointer, &mut out);
    out
}

pub(crate) fn leaf_count(ty: &Type, program: &Program, pointer: types::Type) -> u32 {
    leaves(ty, program, pointer).len() as u32
}

/// The two libc functions the backend imports directly rather than through
/// an `extern fn` declaration.
///
/// They are the console, in both directions (`docs/standard-input.md`).
/// Everything else a program reaches in libc goes through §8.4's `extern`
/// machinery and arrives in `Program::externs`; these two are builtins,
/// so the backend names them itself -- and having two rather than one is
/// what made them worth a name.
#[derive(Clone, Copy)]
pub(crate) struct Console {
    pub(crate) putchar: FuncId,
    pub(crate) getchar: FuncId,
    /// `fwrite(ptr, size, nmemb, stream)` — the bulk write
    /// (`docs/bulk-io.md` §3).
    pub(crate) fwrite: FuncId,
    /// libc's `stdout`, as a *data* symbol to load a `FILE *` out of.
    ///
    /// `fwrite` needs the stream, and it has to be the same stream
    /// `putchar` uses or the two interleave wrongly — which is why this
    /// is `fwrite` and not POSIX `write` on descriptor 1 (§3).
    ///
    /// The symbol is not spelled the same everywhere: glibc exports
    /// `stdout`, and macOS's `stdout` is a macro for `__stdoutp`. Both
    /// of this project's CI targets are here, so both are named.
    pub(crate) stdout: DataId,
    /// libc's `stderr`, the same way (`docs/standard-error.md` §3.3).
    ///
    /// Spelled `stderr` by glibc and `__stderrp` on macOS, exactly as
    /// `stdout` is. C guarantees this stream is not fully buffered, which
    /// is the property §1.2 measured the absence of: a diagnostic written
    /// before a trap has already left.
    pub(crate) stderr: DataId,
}

/// How many leaves a return value may have before it travels through memory.
///
/// Two is what x86-64's SystemV ABI gives back in registers, and Cranelift
/// refuses outright above it. Rather than let the limit differ per target —
/// aarch64 would allow eight — the same rule applies everywhere, so a program
/// that compiles on one target compiles on the other.
pub(crate) const MAX_RETURN_LEAVES: usize = 2;

/// Does a value of this type come back through memory rather than in
/// registers?
pub(crate) fn returns_indirectly(ty: &Type, program: &Program, pointer: types::Type) -> bool {
    leaf_count(ty, program, pointer) as usize > MAX_RETURN_LEAVES
}

/// Byte offset of a leaf in an indirect return buffer.
///
/// One slot of pointer width per leaf: this is a private arrangement between a
/// cancho function and its cancho caller, not a layout the language
/// promises. `docs/defined-behaviour.md` still owns that question in M3, and
/// nothing here is observable to a program.
pub(crate) const RETURN_SLOT_STRIDE: i32 = 8;

/// How much memory one arena takes when it opens (§6).
///
/// A single chunk, obtained once and released once, which is what makes
/// "one pointer reset, no traversal, no per-object bookkeeping" true rather
/// than aspirational. Exhausting it *traps*: the alternative is a chunk list,
/// which turns release into a walk, and the alternative to trapping is
/// undefined behaviour, which the language does not have. Growth without
/// giving up either property is an M3 question, and the trap is what keeps
/// the answer honest until then.
pub(crate) const ARENA_CHUNK: i64 = 64 * 1024;

/// What `bits_of` answers for every NaN: the positive quiet NaN with an
/// empty payload, which is aarch64's default NaN and RISC-V's canonical
/// one. x86-64 generates the same pattern with the sign bit set (its
/// "QNaN floating-point indefinite"), which is the one reason `bits_of`
/// needs this at all.
pub(crate) const CANONICAL_NAN: i64 = 0x7ff8_0000_0000_0000;

/// The same for `bits_of32` (`docs/f32.md` §2): binary32's positive quiet
/// NaN. x86-64 generates it with the sign set, as for binary64.
pub(crate) const CANONICAL_NAN_32: i64 = 0x7fc0_0000;

/// Every cancho function is emitted under this prefix, so a program may define
/// a function called `write` or `exit` without colliding with libc.
pub(crate) const PREFIX: &str = "lexs_";

/// The alignment every word-sized data object asks for (`docs/threads.md`
/// section 6). Cranelift's own default is one, which put the thread
/// counter on an odd address behind glibc's one-byte `completed.0`, and
/// aarch64's exclusive load faults on that. Eight is a leaf (`docs/layout.md`
/// section 1), and what the LLVM backend's typed globals already get.
pub(crate) const WORD_ALIGN: u64 = 8;

/// Where `main` stashes what the runtime handed it
/// (`docs/arguments.md` §3).
///
/// Written once, before any cancho code runs, and never again. `arg_count`
/// and `arg` are the only readers.
pub(crate) const ARGC_GLOBAL: &str = "lexs_argc";
pub(crate) const ARGV_GLOBAL: &str = "lexs_argv";
