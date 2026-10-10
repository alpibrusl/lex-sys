//! The serial-port numbers, in one place so the two backends cannot
//! disagree (`docs/tty.md` §2, every value measured by cancho-robot's
//! spike: `spikes/ping/abi.c` on darwin-aarch64 and
//! `spikes/ping/abi_linux.c` on the robot's Raspberry Pi 5, glibc 2.41).
//!
//! A program writing `extern fn` carries these per platform in source,
//! with no help from the compiler; under `Tty` they are the backend's,
//! written once in Rust. The tables below are the whole reason the
//! capability exists: they differ between the two targets in almost
//! every number, and the macOS half contains the variadic-`ioctl` trap
//! the spike measured (research log 2026-10-08, findings 4 and 9) —
//! declared plainly, `ioctl` answers 0 and configures garbage, which
//! is a silent dead bus rather than an error.

/// `struct termios`'s size and field width, per target.
pub const TERMIOS_SIZE: usize = if cfg!(target_os = "macos") { 72 } else { 60 };

/// `offsetof(struct termios, c_cflag)`: the flags `tty_configure` sets
/// (`CLOCAL | CREAD | CS8` and the speed on Linux).
pub const CFLAG_AT: usize = if cfg!(target_os = "macos") { 16 } else { 8 };

/// `offsetof(struct termios, c_cc)`: where `VMIN` and `VTIME` live.
pub const CC_AT: usize = if cfg!(target_os = "macos") { 32 } else { 17 };

/// `VMIN`'s index in `c_cc`. A nonblocking port does not use it, but the
/// raw-mode settings are incomplete without a defined value.
pub const VMIN: usize = if cfg!(target_os = "macos") { 16 } else { 6 };

/// `VTIME`'s index in `c_cc`, beside `VMIN` for the same reason.
pub const VTIME: usize = if cfg!(target_os = "macos") { 17 } else { 5 };

/// `O_RDWR | O_NOCTTY | O_NONBLOCK`, the flags `tty_open` uses. The
/// nonblocking bit is why `tty_read` never blocks and the poller is the
/// waiting story (`docs/tty.md` §3).
pub const O_RDWR: i64 = 2;
pub const O_NOCTTY: i64 = if cfg!(target_os = "macos") { 131072 } else { 256 };
pub const O_NONBLOCK: i64 = if cfg!(target_os = "macos") { 4 } else { 2048 };

/// `TCSANOW`: configure now, not after drains — a servo bus has nothing
/// to drain and a drain would be a hang.
pub const TCSANOW: i64 = 0;

/// `TCIFLUSH`: what `tty_flush_input` drops. The number differs per
/// target (1 on macOS, 0 on Linux — the spike's tables), which is
/// exactly the kind of constant that does not belong in a program.
pub const TCIFLUSH: i64 = if cfg!(target_os = "macos") { 1 } else { 0 };

/// `CLOCAL | CREAD | CS8`: raw 8N1 without a modem, the one setting the
/// robot needs and the one `tty_configure` makes (`docs/tty.md` §3: no
/// mode algebra).
pub const CLOCAL: i64 = if cfg!(target_os = "macos") { 0x8000 } else { 0x800 };
pub const CREAD: i64 = if cfg!(target_os = "macos") { 0x800 } else { 0x80 };
pub const CS8: i64 = 0x30;

/// `CBAUD`, Linux only: the mask of the speed bits in `c_cflag`, where
/// the standard rates (including 1,000,000) live without an ioctl.
pub const CBAUD: i64 = 0o10017;

/// `cfsetispeed`/`cfsetospeed`'s `B1000000`, Linux only: `010010` octal,
/// a standard rate there (`tcsetattr` alone), while macOS answers
/// `EINVAL` for anything past its standard set and needs the
/// `IOSSIOSPEED` ioctl — the two-step the spike measured.
pub const B1000000: i64 = 0o010010;

/// macOS only: `IOSSIOSPEED`, as **measured** on this target
/// (`0x80085402`, `_IOW('T', 2, speed_t)` with an 8-byte `speed_t`).
/// pyserial 3.5 hard-codes `0x80045402` (a 4-byte size); both are
/// accepted by the observed driver, but the number in the backend is
/// the measured one (`docs/tty.md` §2's note).
#[cfg(target_os = "macos")]
pub const IOSSIOSPEED: i64 = 0x8008_5402;

/// The standard rates both targets accept in `tcsetattr`'s `c_ospeed`,
/// which macOS's two-step needs first (`docs/tty.md` §2): raw mode at
/// one of these, then the `IOSSIOSPEED` ioctl for anything else. The
/// spike used 115200, and it was accepted.
pub const MACOS_STANDARD_STEP: i64 = 115_200;
