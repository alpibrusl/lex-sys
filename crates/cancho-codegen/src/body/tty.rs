//! The serial-port verbs (`docs/tty.md` §3), Cranelift: the termios layout,
//! the B-constants and the macOS two-step live here, written once, instead
//! of in every program's source — the whole reason the capability exists
//! (`cancho-robot`'s spike measured a program carrying all of them, and the
//! variadic-`ioctl` trap that makes a silently dead bus, research log
//! findings 4 and 9).

use crate::body::BodyEmitter;
use crate::*;
use cancho_ir::{
    CC_AT, CFLAG_AT, CLOCAL, CREAD, CS8, O_NOCTTY, O_NONBLOCK, TCIFLUSH, TCSANOW, TERMIOS_SIZE,
    TTY_CFLAG_SPEED, VMIN, VTIME,
};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{MemFlags, Value};

impl<'a, 'f> BodyEmitter<'a, 'f> {
    /// `tty_open(tty, path)` — `openat(AT_FDCWD, path, O_RDWR | O_NOCTTY |
    /// O_NONBLOCK)`, under the capability's prefix, answering a
    /// `TtyOpened`: tag 0 `Ok(Port)` with the descriptor, tag 1
    /// `Failed(errno)` — `open_file`'s tagging, `UdpOpened`'s shape.
    pub(crate) fn tty_open(&mut self, prefix: &str, args: &[cancho_ir::Expr]) -> Vec<Value> {
        let path = self.expr(&args[1]);
        let path = self.checked_path(prefix, &path);
        let flags = 2 | O_NOCTTY | O_NONBLOCK;
        let cwd = self.builder.ins().iconst(types::I32, -100);
        let fd = self.openat(cwd, path, flags, 0);
        let fd = self.builder.ins().sextend(types::I64, fd);
        let failed = self.builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
        let one = self.builder.ins().iconst(types::I64, 1);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let tag = self.builder.ins().select(failed, one, zero);
        let reason = self.errno();
        vec![tag, fd, reason]
    }

    /// `tty_configure(port, baud)` — raw mode, 8N1, and the speed, the one
    /// setting the robot needs (`docs/tty.md` §3: no mode algebra). `0`,
    /// or the `errno` the platform answered.
    ///
    /// The two ABIs (`docs/tty.md` §2, both measured by the spike): Linux
    /// keeps a standard rate in `c_cflag`'s `CBAUD` bits and `tcsetattr`
    /// alone is enough; macOS answers `EINVAL` for a speed past its
    /// standard set, so raw mode is set at a standard rate first and the
    /// speed then goes in by the `IOSSIOSPEED` ioctl — the variadic call
    /// the spike measured failing *silently* when declared plainly. The
    /// target split is made in `cancho_ir::tty`'s constants, so this one
    /// body serves both.
    pub(crate) fn tty_configure(&mut self, args: &[Value]) -> Vec<Value> {
        let pointer = self.pointer;
        let fd = self.handle_fd(args[0]);
        let speed = args[1];

        // The one `struct termios` the backend owns, zeroed on its stack
        // slot (`tcp_listen`'s own-slot shape).
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            TERMIOS_SIZE as u32,
            2,
        ));
        let termios = self.builder.ins().stack_addr(pointer, slot, 0);

        // Raw mode, 8N1, `CLOCAL | CREAD`, `VMIN 0 / VTIME 0` — written
        // into the flags at the offsets the spike measured per target,
        // then applied with `tcsetattr(TCSANOW)`; the standard-rate first
        // step on macOS is the same `tcsetattr` at `MACOS_STANDARD_STEP`,
        // the two-step the spike measured (research log finding 3).
        let cflag = CS8 | CLOCAL | CREAD | TTY_CFLAG_SPEED;
        self.store_word(termios, CFLAG_AT, cflag);
        self.store_word(termios, 0, 0); // c_iflag
        self.store_word(termios, 8, 0); // c_oflag (4-byte fields on Linux are at 0/4/8/12)
        self.store_word(termios, 16, 0); // c_lflag
        self.store_byte(termios, CC_AT + VMIN, 0);
        self.store_byte(termios, CC_AT + VTIME, 0);

        // macOS's first step at a standard rate; on Linux the speed bits
        // are already in `c_cflag` and one `tcsetattr` is the whole
        // configuration (`docs/tty.md` §2's table).
        let now = self.const_i32(TCSANOW);
        let applied = self.libc_call(
            "tcsetattr",
            &[types::I32, types::I32, pointer],
            &[types::I32],
            &[fd, now, termios],
        );
        let bad = self.builder.ins().icmp_imm(IntCC::SignedLessThan, applied, 0);
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I64);
        let refuse = self.builder.create_block();
        let go_on = self.builder.create_block();
        self.builder.ins().brif(bad, refuse, &[], go_on, &[]);

        self.builder.switch_to_block(refuse);
        self.builder.seal_block(refuse);
        let reason = self.errno();
        self.builder.ins().jump(merge, &[reason.into()]);

        self.builder.switch_to_block(go_on);
        self.builder.seal_block(go_on);

        // The speed step, per target: nothing on Linux (the standard rate
        // is already applied); on macOS the `IOSSIOSPEED` ioctl with the
        // measured request number and an 8-byte speed cell — a fixed
        // three-argument call, the variadic trap written once here
        // (`docs/tty.md` §2's note on `0x80085402` vs pyserial's).
        self.tty_apply_speed(fd, speed, termios, merge);

        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().jump(merge, &[zero.into()]);
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        vec![self.builder.block_params(merge)[0]]
    }

    /// The speed step of [`Self::tty_configure`], per target. On Linux
    /// this is nothing: 1,000,000 is a standard rate, in `c_cflag`'s
    /// `CBAUD` bits already applied. On macOS it is the `IOSSIOSPEED`
    /// ioctl on the 8-byte speed cell.
    fn tty_apply_speed(
        &mut self,
        _fd: Value,
        _speed: Value,
        _termios: Value,
        _merge: cranelift_codegen::ir::Block,
    ) {
        if !self.is_darwin() {
            return;
        }
        // macOS: `ioctl(fd, IOSSIOSPEED, &speed)`. The variadic call is
        // shaped the way the callee reads it, as `fcntl`'s is
        // (`native-sockets.md` §3): the pointer in the first stack slot,
        // which is where a variadic argument travels on Apple arm64.
        let pointer = self.pointer;
        let speed_cell = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            8,
            2,
        ));
        let speed_at = self.builder.ins().stack_addr(pointer, speed_cell, 0);
        self.builder.ins().store(MemFlags::trusted(), _speed, speed_at, 0);
        // The measured request (`spikes/ping/abi.c`): 0x80085402, with the
        // 8-byte `speed_t` the header declares — not pyserial's 4-byte
        // constant (`docs/tty.md` §2).
        let request = self.const_i32(0x8008_5402);
        // Six fillers put the pointer in the first stack slot, the ABI
        // the spike proved with its padded declaration (research log
        // finding 4). A hack in a program; the correct shape in a backend.
        let filler = self.builder.ins().iconst(types::I64, 0);
        let mut params = vec![types::I32, types::I32];
        params.extend([types::I64; 6]);
        params.push(pointer);
        let mut args = vec![_fd, request];
        args.extend([filler; 5]);
        args.push(speed_at);
        let _ = self.libc_call("ioctl", &params, &[types::I32], &args);
    }

    /// `tty_read(port, into)` — one `read(2)`, never blocks
    /// (`O_NONBLOCK`; the poller is the waiting story, `docs/tty.md` §3).
    /// `-1` on error, the `Conn` verbs' convention.
    pub(crate) fn tty_read(&mut self, args: &[Value]) -> Vec<Value> {
        let pointer = self.pointer;
        let fd = self.handle_fd(args[0]);
        vec![self.libc_call(
            "read",
            &[types::I32, pointer, types::I64],
            &[types::I64],
            &[fd, args[1], args[2]],
        )]
    }

    /// `tty_write(port, bytes)` — one `write(2)`, whole or short, on a
    /// shared reference as `conn_write`'s is. `-1` on error.
    pub(crate) fn tty_write(&mut self, args: &[Value]) -> Vec<Value> {
        let pointer = self.pointer;
        let fd = self.handle_fd(args[0]);
        vec![self.libc_call(
            "write",
            &[types::I32, pointer, types::I64],
            &[types::I64],
            &[fd, args[1], args[2]],
        )]
    }

    /// `tty_flush_input(port)` — `tcflush(fd, TCIFLUSH)`. The queue
    /// selector's number differs per target (1 on macOS, 0 on Linux —
    /// `docs/tty.md` §2), exactly a constant that does not belong in a
    /// program.
    pub(crate) fn tty_flush_input(&mut self, args: &[Value]) -> Vec<Value> {
        let fd = self.handle_fd(args[0]);
        let queue = self.const_i32(TCIFLUSH);
        let flushed =
            self.libc_call("tcflush", &[types::I32, types::I32], &[types::I32], &[fd, queue]);
        vec![self.builder.ins().sextend(types::I64, flushed)]
    }

    // ---- the target-split stores ----------------------------------------

    /// A flags field of `struct termios`, at the target's own width and
    /// offset: 4 bytes on Linux, 8 on macOS (`docs/tty.md` §2's table —
    /// the difference that makes these the backend's, not a program's).
    fn store_word(&mut self, base: Value, at: usize, value: i64) {
        let width = if cfg!(target_os = "macos") { 8 } else { 4 };
        let stored = self.builder.ins().iconst(types::I64, value);
        match width {
            8 => {
                self.builder.ins().store(MemFlags::trusted(), stored, base, at as i32);
            }
            _ => {
                let narrowed = self.builder.ins().ireduce(types::I32, stored);
                self.builder.ins().store(MemFlags::trusted(), narrowed, base, at as i32);
            }
        }
    }

    /// One byte of `c_cc`, at the target's own index.
    fn store_byte(&mut self, base: Value, at: usize, value: i64) {
        let stored = self.builder.ins().iconst(types::I32, value);
        let narrowed = self.builder.ins().ireduce(types::I8, stored);
        self.builder.ins().store(MemFlags::trusted(), narrowed, base, at as i32);
    }

    fn const_i32(&mut self, value: i64) -> Value {
        self.builder.ins().iconst(types::I32, value)
    }
}
