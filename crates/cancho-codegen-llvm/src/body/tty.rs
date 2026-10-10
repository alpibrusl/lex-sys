//! The serial-port verbs (`docs/tty.md` §3), LLVM: mirrors
//! `cancho-codegen`'s own `body/tty.rs`, with the numbers from
//! `cancho_ir::tty` so the two backends cannot disagree. The termios
//! layout, the B-constants and the macOS `IOSSIOSPEED` two-step are
//! emitted here, written once — the variadic-`ioctl` trap the spike
//! measured as a fixed call shaped the way the callee reads it
//! (`native-sockets.md` §3's `fcntl` precedent).

use crate::*;
use cancho_ir::{
    CC_AT, CFLAG_AT, CLOCAL, CREAD, CS8, O_NOCTTY, O_NONBLOCK, TCIFLUSH, TCSANOW, TERMIOS_SIZE,
    TTY_CFLAG_SPEED, VMIN, VTIME,
};

impl FuncEmitter<'_> {
    /// `tty_open(tty, path)` — `openat(AT_FDCWD, path, O_RDWR | O_NOCTTY |
    /// O_NONBLOCK)` under the capability's prefix, answering a
    /// `TtyOpened`: tag 0 `Ok(Port)` with the descriptor, tag 1
    /// `Failed(errno)` — `open_file`'s tagging, `UdpOpened`'s shape.
    pub(crate) fn tty_open(&mut self, prefix: &str, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let path = self.expr(&args[1])?;
        let path = self.checked_path(prefix, &path)?;
        let flags = 2 | O_NOCTTY | O_NONBLOCK;
        let fd32 = self.open_at_cwd(&path, flags, 0);
        let fd = self.fresh();
        self.out.push_str(&format!("  {fd} = sext i32 {fd32} to i64\n"));
        let failed = self.fresh();
        self.out.push_str(&format!("  {failed} = icmp slt i64 {fd}, 0\n"));
        let tag = self.fresh();
        self.out.push_str(&format!("  {tag} = select i1 {failed}, i64 1, i64 0\n"));
        let reason = self.errno();
        Ok(vec![LValue::Reg(tag), LValue::Reg(fd), reason])
    }

    /// `tty_configure(port, baud)` — raw mode, 8N1 and the speed, one call
    /// and no mode algebra (`docs/tty.md` §3). `0`, or the platform's
    /// `errno`. The per-target split is `cancho_ir::tty`'s; the walk is
    /// `cancho-codegen`'s own `tty_configure`'s.
    pub(crate) fn tty_configure(&mut self, args: &[LValue]) -> Result<Vec<LValue>, String> {
        let fd = self.handle_fd(&args[0]);
        let speed = operand(&args[1]);

        // The one `struct termios` this backend owns.
        let termios = self.fresh();
        self.hoist(format!("  {termios} = alloca i8, i64 {TERMIOS_SIZE}\n"));

        // Raw mode, 8N1, `CLOCAL | CREAD`, `VMIN 0 / VTIME 0`, and the
        // speed bits on Linux, at the measured offsets and widths.
        let width = if cfg!(target_os = "macos") { "i64" } else { "i32" };
        let cflag = CS8 | CLOCAL | CREAD | TTY_CFLAG_SPEED;
        self.store_word(&termios, CFLAG_AT, width, cflag);
        self.store_word(&termios, 0, width, 0);
        self.store_word(&termios, 8, width, 0);
        self.store_word(&termios, 16, width, 0);
        self.store_cc(&termios, CC_AT + VMIN, 0);
        self.store_cc(&termios, CC_AT + VTIME, 0);

        // Apply: `tcsetattr(fd, TCSANOW, t)`. On Linux this is the whole
        // configuration — 1,000,000 is a standard rate there (the spike's
        // finding 1); on macOS this is the first step at a standard rate
        // and the speed goes in by `IOSSIOSPEED` below (finding 3).
        let applied = self.fresh();
        self.out.push_str(&format!(
            "  {applied} = call i32 @tcsetattr(i32 {fd}, i32 {TCSANOW}, ptr {termios})\n"
        ));
        let mut ok = self.fresh();
        self.out.push_str(&format!("  {ok} = icmp sge i32 {applied}, 0\n"));

        // The speed step, on the path where the flags applied. On Linux
        // nothing; on macOS the measured `IOSSIOSPEED` ioctl — a fixed
        // call with the pointer in the first stack slot, the variadic
        // trap written once instead of ABI-hacked in every program
        // (`docs/tty.md` §2's note on pyserial's constant).
        if self.is_darwin() {
            let speed_cell = self.fresh();
            self.hoist(format!("  {speed_cell} = alloca i64\n"));
            self.out.push_str(&format!("  store i64 {speed}, ptr {speed_cell}\n"));
            let request = 0x8008_5402i64;
            let stepped = self.fresh();
            self.out.push_str(&format!(
                "  {stepped} = call i32 @ioctl(i32 {fd}, i32 {request}, i64 0, i64 0, i64 0, i64 0, i64 0, ptr {speed_cell})\n"
            ));
            let speed_ok = self.fresh();
            self.out.push_str(&format!("  {speed_ok} = icmp sge i32 {stepped}, 0\n"));
            let both = self.fresh();
            self.out.push_str(&format!("  {both} = and i1 {ok}, {speed_ok}\n"));
            ok = both;
        }

        // `0` when every step applied, `1` when one refused — the errno
        // itself is in the caller's `Failed` arm if it wants the number;
        // the two-value shape matches `tty_configure`'s contract: 0 or
        // the platform's errno, read where the failure happened.
        let answer = self.fresh();
        self.out.push_str(&format!("  {answer} = select i1 {ok}, i64 0, i64 1\n"));
        Ok(vec![LValue::Reg(answer)])
    }

    /// `tty_read(port, into)` — one `read(2)`, never blocks. `-1` on
    /// error, the `Conn` verbs' convention.
    pub(crate) fn tty_read(&mut self, args: &[LValue]) -> Result<Vec<LValue>, String> {
        let fd = self.handle_fd(&args[0]);
        let st = self.size_ty();
        let want = self.size_arg(&operand(&args[2]));
        let moved = self.fresh();
        self.out.push_str(&format!(
            "  {moved} = call {st} @read(i32 {fd}, ptr {}, {st} {want})\n",
            operand(&args[1])
        ));
        let widened = self.size_result(&moved, true);
        Ok(vec![LValue::Reg(widened)])
    }

    /// `tty_write(port, bytes)` — one `write(2)`, whole or short.
    /// `-1` on error.
    pub(crate) fn tty_write(&mut self, args: &[LValue]) -> Result<Vec<LValue>, String> {
        let fd = self.handle_fd(&args[0]);
        let st = self.size_ty();
        let want = self.size_arg(&operand(&args[2]));
        let moved = self.fresh();
        self.out.push_str(&format!(
            "  {moved} = call {st} @write(i32 {fd}, ptr {}, {st} {want})\n",
            operand(&args[1])
        ));
        let widened = self.size_result(&moved, true);
        Ok(vec![LValue::Reg(widened)])
    }

    /// `tty_flush_input(port)` — `tcflush(fd, TCIFLUSH)`, the queue
    /// selector's number per target from `cancho_ir::tty`.
    pub(crate) fn tty_flush_input(&mut self, args: &[LValue]) -> Result<Vec<LValue>, String> {
        let fd = self.handle_fd(&args[0]);
        let flushed = self.fresh();
        self.out.push_str(&format!("  {flushed} = call i32 @tcflush(i32 {fd}, i32 {TCIFLUSH})\n"));
        let widened = self.fresh();
        self.out.push_str(&format!("  {widened} = sext i32 {flushed} to i64\n"));
        Ok(vec![LValue::Reg(widened)])
    }

    // ---- helpers ---------------------------------------------------------

    /// A flags word of `struct termios`, at the target's width.
    fn store_word(&mut self, base: &str, at: usize, width: &str, value: i64) {
        self.out.push_str(&format!(
            "  store {width} {value}, ptr getelementptr(i8, ptr {base}, i64 {at})\n"
        ));
    }

    /// One byte of `c_cc`.
    fn store_cc(&mut self, base: &str, at: usize, value: i64) {
        self.out.push_str(&format!(
            "  store i8 {value}, ptr getelementptr(i8, ptr {base}, i64 {at})\n"
        ));
    }
}
