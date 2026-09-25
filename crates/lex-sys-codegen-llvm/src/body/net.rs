//! `bind` (`docs/net.md` §2.1, `docs/listen.md` §6): the second of
//! `Net`'s four builtins this backend lowers, after `listen`/`accept`
//! (§7.20). `connect` -- the other half, needing `getaddrinfo`-based host
//! resolution -- is still outside this backend.

use crate::*;

/// A bound's port half, read at compile time: `None` for no restriction,
/// `Some(port)` for one that names a port. Mirrors `lex-sys-codegen`'s own
/// `port_bound_of` (`body/net.rs`) exactly -- a bound whose port half does
/// not parse as a number reads as `-1`, a port `bind` can never be asked
/// for, rather than as `None`, so a narrowing that failed to *tighten* as
/// intended still enforces something (`docs/listen.md` §6.2).
fn port_bound_of(text: &str) -> Option<i64> {
    if text.is_empty() { None } else { Some(text.parse().unwrap_or(-1)) }
}

impl<'a> FuncEmitter<'a> {
    /// Store one byte -- a constant (`"2"`) or a register (`%t3`) -- at
    /// `base + offset`.
    fn store_byte(&mut self, base: &str, offset: i32, byte: &str) {
        let addr = self.fresh();
        self.out.push_str(&format!("  {addr} = getelementptr i8, ptr {base}, i64 {offset}\n"));
        self.out.push_str(&format!("  store i8 {byte}, ptr {addr}\n"));
    }

    /// `bind(net, port)` (`docs/listen.md` §6): the inbound mirror of the
    /// still-unbuilt `connect`. Folds `socket`, `setsockopt(SO_REUSEADDR)`
    /// and `bind` into one call, building the same `struct sockaddr_in`
    /// `lex-sys-codegen`'s own `bind` builds by hand and
    /// `examples/serve/serve.ls` builds by hand again -- family bytes, the
    /// port big-endian, then `INADDR_ANY`: eight zero bytes where
    /// `connect`'s own has four octets, because a listener binds every
    /// address the host has. `args` is the capability (zero-sized,
    /// stopping here) and the port.
    ///
    /// Unlike `Sqrt`/`Listen`/`Accept`, this cannot be one straight-line
    /// call: `socket`/`bind` can each fail, and a failure returns `-1`
    /// rather than trapping (only a bound mismatch traps). This backend
    /// builds no `phi`, the same rule `if_stmt`/`while_stmt` already
    /// follow, so the two failure paths and the success path each store
    /// into one `alloca i64` result cell instead of merging through a
    /// block parameter.
    pub(crate) fn bind(&mut self, bound: &str, args: &[Expr]) -> Result<Vec<LValue>, String> {
        let port = self.scalar(&args[1])?;

        // §6.1: the bound is the port alone, not `"host:port"`, so there
        // is no host half to split off first.
        if let Some(expected) = port_bound_of(bound) {
            let wrong_port = self.fresh();
            self.out.push_str(&format!(
                "  {wrong_port} = icmp ne i64 {}, {expected}\n",
                operand(&port)
            ));
            self.trap_if(&wrong_port)?;
        }

        let addr = self.fresh();
        self.out.push_str(&format!("  {addr} = alloca i8, i64 16\n"));
        // Family bytes -- `2, 0` -- the same choice `connect`'s own
        // `docs/connect.md` §3 measured: BSD kernels read family `0` as
        // `AF_INET` too, for backward compatibility, so writing only the
        // Linux byte layout works on both targets with no branch.
        self.store_byte(&addr, 0, "2");
        self.store_byte(&addr, 1, "0");
        let port32 = self.fresh();
        self.out.push_str(&format!("  {port32} = trunc i64 {} to i32\n", operand(&port)));
        let high32 = self.fresh();
        self.out.push_str(&format!("  {high32} = lshr i32 {port32}, 8\n"));
        let high8 = self.fresh();
        self.out.push_str(&format!("  {high8} = trunc i32 {high32} to i8\n"));
        let low8 = self.fresh();
        self.out.push_str(&format!("  {low8} = trunc i32 {port32} to i8\n"));
        self.store_byte(&addr, 2, &high8);
        self.store_byte(&addr, 3, &low8);
        // `INADDR_ANY`: every remaining byte, including the address
        // itself, is zero.
        for offset in 4..16 {
            self.store_byte(&addr, offset, "0");
        }

        let fd = self.fresh();
        self.out.push_str(&format!("  {fd} = call i32 @socket(i32 2, i32 1, i32 0)\n"));

        let result_cell = self.fresh();
        self.out.push_str(&format!("  {result_cell} = alloca i64\n"));
        let minus_one = LValue::Const(-1);

        let bad_socket = self.fresh();
        self.out.push_str(&format!("  {bad_socket} = icmp slt i32 {fd}, 0\n"));
        let n = self.blocks;
        self.blocks += 1;
        let (no_socket, have_socket, bound_ok, bind_failed, merge) = (
            format!("nosocket{n}"),
            format!("havesocket{n}"),
            format!("boundok{n}"),
            format!("bindfailed{n}"),
            format!("bindmerge{n}"),
        );
        self.out
            .push_str(&format!("  br i1 {bad_socket}, label %{no_socket}, label %{have_socket}\n"));

        self.out.push_str(&format!("{no_socket}:\n"));
        self.out.push_str(&format!("  store i64 {}, ptr {result_cell}\n", operand(&minus_one)));
        self.out.push_str(&format!("  br label %{merge}\n"));

        self.out.push_str(&format!("{have_socket}:\n"));
        // `SOL_SOCKET` (1), `SO_REUSEADDR` (2): a C `int`, four bytes,
        // least significant first, the same value `serve.ls` assembles by
        // hand (`docs/reach.md` §3.2).
        let reuse = self.fresh();
        self.out.push_str(&format!("  {reuse} = alloca i8, i64 4\n"));
        self.store_byte(&reuse, 0, "1");
        self.store_byte(&reuse, 1, "0");
        self.store_byte(&reuse, 2, "0");
        self.store_byte(&reuse, 3, "0");
        self.out.push_str(&format!(
            "  call i32 @setsockopt(i32 {fd}, i32 1, i32 2, ptr {reuse}, i32 4)\n"
        ));

        let bind_result = self.fresh();
        self.out
            .push_str(&format!("  {bind_result} = call i32 @bind(i32 {fd}, ptr {addr}, i32 16)\n"));
        let ok = self.fresh();
        self.out.push_str(&format!("  {ok} = icmp eq i32 {bind_result}, 0\n"));
        self.out.push_str(&format!("  br i1 {ok}, label %{bound_ok}, label %{bind_failed}\n"));

        self.out.push_str(&format!("{bound_ok}:\n"));
        let fd64 = self.fresh();
        self.out.push_str(&format!("  {fd64} = sext i32 {fd} to i64\n"));
        self.out.push_str(&format!("  store i64 {fd64}, ptr {result_cell}\n"));
        self.out.push_str(&format!("  br label %{merge}\n"));

        self.out.push_str(&format!("{bind_failed}:\n"));
        self.out.push_str(&format!("  call i32 @close(i32 {fd})\n"));
        self.out.push_str(&format!("  store i64 {}, ptr {result_cell}\n", operand(&minus_one)));
        self.out.push_str(&format!("  br label %{merge}\n"));

        self.out.push_str(&format!("{merge}:\n"));
        let result = self.fresh();
        self.out.push_str(&format!("  {result} = load i64, ptr {result_cell}\n"));
        Ok(vec![LValue::Reg(result)])
    }
}
