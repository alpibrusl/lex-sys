//! `connect` and `bind` (`docs/net.md` §4.1, §2.1; `docs/connect.md`
//! §10; `docs/listen.md` §6).

use crate::*;

/// `struct addrinfo`'s byte offsets that agree on every target this
/// project supports: POSIX's `<netdb.h>` fixes the four `int`s and
/// `ai_addrlen` first, and `ai_next` last, the same way on glibc and on
/// Darwin's libc.
const AI_FLAGS: i32 = 0;
const AI_FAMILY: i32 = 4;
const AI_SOCKTYPE: i32 = 8;
const AI_PROTOCOL: i32 = 12;
const AI_ADDRLEN: i32 = 16;
const AI_NEXT: i32 = 40;
const ADDRINFO_SIZE: u32 = 48;

/// A bound's port half, read at compile time: `None` for no restriction
/// (an empty half -- an unnarrowed bound, or `net.md` §2.1's outbound
/// bound with no `:` at all), `Some(port)` for one that names a port.
///
/// A half that is not empty and does not parse -- `narrow` never checks
/// that a bound's text is numeric -- reads as `-1`, a port `connect`/
/// `bind` can never be asked to dial, rather than as `None`: a malformed
/// bound is not the same fact as an absent one, and reading them alike
/// would let a narrowing that failed to *tighten* as intended enforce
/// nothing instead (`docs/listen.md` §6.2).
fn port_bound_of(text: &str) -> Option<i64> {
    if text.is_empty() { None } else { Some(text.parse().unwrap_or(-1)) }
}

impl<'a, 'f> BodyEmitter<'a, 'f> {
    /// The bound a `Net` was narrowed to, checked against a dialled name at
    /// run time and NUL-terminated for `getaddrinfo` — `checked_path`'s
    /// shape, minus the `/`-boundary and `..` rules neither `Net` nor a
    /// host name has (`docs/connect.md` §10.1).
    pub(crate) fn checked_host(&mut self, bound: &str, name: &[Value]) -> Value {
        const HOST_MAX: i64 = 256;
        let pointer = self.pointer;
        let (source, length) = (name[0], name[1]);

        let too_long =
            self.builder.ins().icmp_imm(IntCC::UnsignedGreaterThanOrEqual, length, HOST_MAX);
        self.builder.ins().trapnz(too_long, TrapCode::HEAP_OUT_OF_BOUNDS);

        let short =
            self.builder.ins().icmp_imm(IntCC::UnsignedLessThan, length, bound.len() as i64);
        self.builder.ins().trapnz(short, TrapCode::HEAP_OUT_OF_BOUNDS);

        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            HOST_MAX as u32,
            0,
        ));
        let buffer = self.builder.ins().stack_addr(pointer, slot, 0);
        let expected = self.bytes(bound)[0];

        let header = self.builder.create_block();
        let body = self.builder.create_block();
        let done = self.builder.create_block();
        let cursor = self.temporary(types::I64);
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.def_var(cursor, zero);
        self.builder.ins().jump(header, &[]);

        self.builder.switch_to_block(header);
        let i = self.builder.use_var(cursor);
        let more = self.builder.ins().icmp(IntCC::UnsignedLessThan, i, length);
        self.builder.ins().brif(more, body, &[], done, &[]);

        self.builder.switch_to_block(body);
        self.builder.seal_block(body);
        let i = self.builder.use_var(cursor);
        let at = self.builder.ins().iadd(source, i);
        let byte = self.builder.ins().load(types::I8, MemFlags::trusted(), at, 0);
        let into = self.builder.ins().iadd(buffer, i);
        self.builder.ins().store(MemFlags::trusted(), byte, into, 0);

        // Inside the bound, the bytes have to match -- a plain prefix, with
        // no separator to land on (`docs/net.md` §4, `lower/mod.rs`'s
        // `narrow`).
        let inside = self.builder.ins().icmp_imm(IntCC::UnsignedLessThan, i, bound.len() as i64);
        let want_at = self.builder.ins().iadd(expected, i);
        let want = self.builder.ins().load(types::I8, MemFlags::trusted(), want_at, 0);
        let differs = self.builder.ins().icmp(IntCC::NotEqual, byte, want);
        let escaped = self.builder.ins().band(inside, differs);
        self.builder.ins().trapnz(escaped, TrapCode::HEAP_OUT_OF_BOUNDS);

        let next = self.builder.ins().iadd_imm(i, 1);
        self.builder.def_var(cursor, next);
        self.builder.ins().jump(header, &[]);
        self.builder.seal_block(header);

        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        let none = self.builder.ins().iconst(types::I8, 0);
        let end = self.builder.ins().iadd(buffer, length);
        self.builder.ins().store(MemFlags::trusted(), none, end, 0);
        buffer
    }

    /// `connect(net, name, port)` (`docs/connect.md` §10): checks `name`
    /// against the bound, resolves it with `getaddrinfo`, patches the port
    /// into whatever `sockaddr` the resolver filled in, and connects.
    /// `args` is the capability (zero-sized, stopping here), the name as
    /// `&r [byte]`, and the port as an `int`.
    pub(crate) fn connect(&mut self, bound: &str, args: &[Expr]) -> Vec<Value> {
        let pointer = self.pointer;
        let name = self.expr(&args[1]);
        let port = self.scalar(&args[2]);

        // The bound is `"host:port"` (`docs/net.md` §4); split once, at
        // compile time, since the bound is known then and neither half
        // needs a runtime parse. The host half is a prefix check, the same
        // shape `checked_path` already is; the port half is a narrower
        // capability naming one exact port, so it is equality, not a
        // prefix (`docs/connect.md` §10.1). A bound with no `:` -- `""`,
        // unnarrowed, included -- restricts the port to none.
        let (host_bound, port_bound) = match bound.rsplit_once(':') {
            Some((host, digits)) => (host, port_bound_of(digits)),
            None => (bound, None),
        };
        if let Some(expected) = port_bound {
            let wrong_port = self.builder.ins().icmp_imm(IntCC::NotEqual, port, expected);
            self.builder.ins().trapnz(wrong_port, TrapCode::HEAP_OUT_OF_BOUNDS);
        }
        let host = self.checked_host(host_bound, &name);

        // `ai_addr` and `ai_canonname` swap places between glibc and
        // Darwin's libc -- BSD's original `getaddrinfo` (RFC 2553) put
        // `ai_canonname` first, and glibc did not follow it. Every other
        // field agrees (`docs/connect.md` §10.2's table), which is why
        // only these two need a target check, the same way `errno`'s
        // symbol name already does.
        let (ai_addr, ai_canonname) = match self.module.isa().triple().operating_system {
            target_lexicon::OperatingSystem::Darwin(_) => (32, 24),
            _ => (24, 32),
        };

        // `struct addrinfo hints`, zeroed except the two fields that ask
        // for one address family and one socket kind -- IPv4 and TCP, the
        // only shape this project has ever built a `sockaddr_in` for.
        let hints_slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            ADDRINFO_SIZE,
            0,
        ));
        let hints = self.builder.ins().stack_addr(pointer, hints_slot, 0);
        let zero32 = self.builder.ins().iconst(types::I32, 0);
        let af_inet = self.builder.ins().iconst(types::I32, 2);
        let sock_stream = self.builder.ins().iconst(types::I32, 1);
        let null = self.builder.ins().iconst(pointer, 0);
        self.builder.ins().store(MemFlags::trusted(), zero32, hints, AI_FLAGS);
        self.builder.ins().store(MemFlags::trusted(), af_inet, hints, AI_FAMILY);
        self.builder.ins().store(MemFlags::trusted(), sock_stream, hints, AI_SOCKTYPE);
        self.builder.ins().store(MemFlags::trusted(), zero32, hints, AI_PROTOCOL);
        self.builder.ins().store(MemFlags::trusted(), zero32, hints, AI_ADDRLEN);
        self.builder.ins().store(MemFlags::trusted(), null, hints, ai_addr);
        self.builder.ins().store(MemFlags::trusted(), null, hints, ai_canonname);
        self.builder.ins().store(MemFlags::trusted(), null, hints, AI_NEXT);

        let res_slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            pointer.bytes(),
            0,
        ));
        let res_addr = self.builder.ins().stack_addr(pointer, res_slot, 0);

        let getaddrinfo =
            self.libc_fn("getaddrinfo", &[pointer, pointer, pointer, pointer], &[types::I32]);
        let getaddrinfo = self.module.declare_func_in_func(getaddrinfo, self.builder.func);
        // `service` is always `NULL`: the port is read out of the bound's
        // own text and patched in afterward, not looked up by name
        // (§10.2's reasons).
        let no_service = self.builder.ins().iconst(pointer, 0);
        let call = self.builder.ins().call(getaddrinfo, &[host, no_service, hints, res_addr]);
        let resolve_result = self.builder.inst_results(call)[0];

        let minus_one = self.builder.ins().iconst(types::I64, -1);
        let resolved = self.builder.create_block();
        let not_resolved = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I64);
        let failed_resolve = self.builder.ins().icmp_imm(IntCC::NotEqual, resolve_result, 0);
        self.builder.ins().brif(failed_resolve, not_resolved, &[], resolved, &[]);

        self.builder.switch_to_block(not_resolved);
        self.builder.seal_block(not_resolved);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(resolved);
        self.builder.seal_block(resolved);
        // The first result is enough: this program connects once, and
        // every candidate is the one host it asked for.
        let res = self.builder.ins().load(pointer, MemFlags::trusted(), res_addr, 0);
        let addr = self.builder.ins().load(pointer, MemFlags::trusted(), res, ai_addr);
        let addrlen = self.builder.ins().load(types::I32, MemFlags::trusted(), res, AI_ADDRLEN);
        let freeaddrinfo = self.libc_fn("freeaddrinfo", &[pointer], &[]);
        let freeaddrinfo = self.module.declare_func_in_func(freeaddrinfo, self.builder.func);

        // The port, big-endian, at the one offset `connect.md` §3 found
        // portable: bytes 0-1 are where Linux and macOS disagree, and
        // `getaddrinfo` already wrote the platform's own correct bytes
        // there, so nothing here has to know which one it is running on.
        let port32 = self.builder.ins().ireduce(types::I32, port);
        let high = self.builder.ins().ushr_imm(port32, 8);
        let high = self.builder.ins().ireduce(types::I8, high);
        let low = self.builder.ins().ireduce(types::I8, port32);
        self.builder.ins().store(MemFlags::trusted(), high, addr, 2);
        self.builder.ins().store(MemFlags::trusted(), low, addr, 3);

        let socket = self.libc_fn("socket", &[types::I32, types::I32, types::I32], &[types::I32]);
        let socket = self.module.declare_func_in_func(socket, self.builder.func);
        let domain = self.builder.ins().iconst(types::I32, 2);
        let kind = self.builder.ins().iconst(types::I32, 1);
        let proto = self.builder.ins().iconst(types::I32, 0);
        let call = self.builder.ins().call(socket, &[domain, kind, proto]);
        let fd = self.builder.inst_results(call)[0];

        let no_socket = self.builder.create_block();
        let have_socket = self.builder.create_block();
        let bad_socket = self.builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
        self.builder.ins().brif(bad_socket, no_socket, &[], have_socket, &[]);

        self.builder.switch_to_block(no_socket);
        self.builder.seal_block(no_socket);
        self.builder.ins().call(freeaddrinfo, &[res]);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(have_socket);
        self.builder.seal_block(have_socket);
        let connect = self.libc_fn("connect", &[types::I32, pointer, types::I32], &[types::I32]);
        let connect = self.module.declare_func_in_func(connect, self.builder.func);
        let call = self.builder.ins().call(connect, &[fd, addr, addrlen]);
        let result = self.builder.inst_results(call)[0];

        let connected = self.builder.create_block();
        let not_connected = self.builder.create_block();
        let ok = self.builder.ins().icmp_imm(IntCC::Equal, result, 0);
        self.builder.ins().brif(ok, connected, &[], not_connected, &[]);

        self.builder.switch_to_block(connected);
        self.builder.seal_block(connected);
        self.builder.ins().call(freeaddrinfo, &[res]);
        let fd64 = self.builder.ins().sextend(types::I64, fd);
        self.builder.ins().jump(merge, &[fd64.into()]);

        self.builder.switch_to_block(not_connected);
        self.builder.seal_block(not_connected);
        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
        let close = self.module.declare_func_in_func(close, self.builder.func);
        self.builder.ins().call(close, &[fd]);
        self.builder.ins().call(freeaddrinfo, &[res]);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        vec![self.builder.block_params(merge)[0]]
    }

    /// `bind(net, port)` (`docs/listen.md` §6): the inbound mirror of
    /// [`Self::connect`]. Folds `socket`, `setsockopt(SO_REUSEADDR)` and
    /// `bind` into one call, the way `examples/serve/`'s own `serve`
    /// builds the same `struct sockaddr_in` by hand -- family bytes, the
    /// port big-endian, then `INADDR_ANY`: eight zero bytes where
    /// `connect`'s has four octets, because a listener binds every
    /// address the host has. `args` is the capability (zero-sized,
    /// stopping here) and the port.
    pub(crate) fn bind(&mut self, bound: &str, args: &[Expr]) -> Vec<Value> {
        let pointer = self.pointer;
        let port = self.scalar(&args[1]);

        // §6.1: the bound is the port alone, not `"host:port"`, so there
        // is no host half to split off first.
        if let Some(expected) = port_bound_of(bound) {
            let wrong_port = self.builder.ins().icmp_imm(IntCC::NotEqual, port, expected);
            self.builder.ins().trapnz(wrong_port, TrapCode::HEAP_OUT_OF_BOUNDS);
        }

        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            0,
        ));
        let addr = self.builder.ins().stack_addr(pointer, slot, 0);
        let zero8 = self.builder.ins().iconst(types::I8, 0);
        let family = self.builder.ins().iconst(types::I8, 2);
        self.builder.ins().store(MemFlags::trusted(), family, addr, 0);
        self.builder.ins().store(MemFlags::trusted(), zero8, addr, 1);
        let port32 = self.builder.ins().ireduce(types::I32, port);
        let high = self.builder.ins().ushr_imm(port32, 8);
        let high = self.builder.ins().ireduce(types::I8, high);
        let low = self.builder.ins().ireduce(types::I8, port32);
        self.builder.ins().store(MemFlags::trusted(), high, addr, 2);
        self.builder.ins().store(MemFlags::trusted(), low, addr, 3);
        // `INADDR_ANY`: every remaining byte, including the address
        // itself, is zero.
        for i in 4..16i32 {
            self.builder.ins().store(MemFlags::trusted(), zero8, addr, i);
        }

        let socket = self.libc_fn("socket", &[types::I32, types::I32, types::I32], &[types::I32]);
        let socket = self.module.declare_func_in_func(socket, self.builder.func);
        let domain = self.builder.ins().iconst(types::I32, 2);
        let kind = self.builder.ins().iconst(types::I32, 1);
        let proto = self.builder.ins().iconst(types::I32, 0);
        let call = self.builder.ins().call(socket, &[domain, kind, proto]);
        let fd = self.builder.inst_results(call)[0];
        let minus_one = self.builder.ins().iconst(types::I64, -1);

        let no_socket = self.builder.create_block();
        let have_socket = self.builder.create_block();
        let merge = self.builder.create_block();
        self.builder.append_block_param(merge, types::I64);
        let bad_socket = self.builder.ins().icmp_imm(IntCC::SignedLessThan, fd, 0);
        self.builder.ins().brif(bad_socket, no_socket, &[], have_socket, &[]);

        self.builder.switch_to_block(no_socket);
        self.builder.seal_block(no_socket);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(have_socket);
        self.builder.seal_block(have_socket);
        // `SOL_SOCKET` (1), `SO_REUSEADDR` (2): a C `int`, four bytes,
        // least significant first, the same value `serve.ls` assembles by
        // hand (`docs/reach.md` §3.2).
        let reuse_slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            4,
            0,
        ));
        let reuse = self.builder.ins().stack_addr(pointer, reuse_slot, 0);
        let one8 = self.builder.ins().iconst(types::I8, 1);
        self.builder.ins().store(MemFlags::trusted(), one8, reuse, 0);
        self.builder.ins().store(MemFlags::trusted(), zero8, reuse, 1);
        self.builder.ins().store(MemFlags::trusted(), zero8, reuse, 2);
        self.builder.ins().store(MemFlags::trusted(), zero8, reuse, 3);
        let setsockopt = self.libc_fn(
            "setsockopt",
            &[types::I32, types::I32, types::I32, pointer, types::I32],
            &[types::I32],
        );
        let setsockopt = self.module.declare_func_in_func(setsockopt, self.builder.func);
        let sol_socket = self.builder.ins().iconst(types::I32, 1);
        let so_reuseaddr = self.builder.ins().iconst(types::I32, 2);
        let reuse_len = self.builder.ins().iconst(types::I32, 4);
        self.builder.ins().call(setsockopt, &[fd, sol_socket, so_reuseaddr, reuse, reuse_len]);

        let bind = self.libc_fn("bind", &[types::I32, pointer, types::I32], &[types::I32]);
        let bind = self.module.declare_func_in_func(bind, self.builder.func);
        let len = self.builder.ins().iconst(types::I32, 16);
        let call = self.builder.ins().call(bind, &[fd, addr, len]);
        let result = self.builder.inst_results(call)[0];

        let bound_ok = self.builder.create_block();
        let bind_failed = self.builder.create_block();
        let ok = self.builder.ins().icmp_imm(IntCC::Equal, result, 0);
        self.builder.ins().brif(ok, bound_ok, &[], bind_failed, &[]);

        self.builder.switch_to_block(bound_ok);
        self.builder.seal_block(bound_ok);
        let fd64 = self.builder.ins().sextend(types::I64, fd);
        self.builder.ins().jump(merge, &[fd64.into()]);

        self.builder.switch_to_block(bind_failed);
        self.builder.seal_block(bind_failed);
        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
        let close = self.module.declare_func_in_func(close, self.builder.func);
        self.builder.ins().call(close, &[fd]);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        vec![self.builder.block_params(merge)[0]]
    }
}
