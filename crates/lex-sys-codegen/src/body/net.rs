//! `connect` (`docs/net.md` §4.1, `docs/connect.md`).

use crate::*;

impl<'a, 'f> BodyEmitter<'a, 'f> {
    /// `connect(net, a, b, c, d, port)` — slice 1 (`docs/connect.md` §1):
    /// the address is already four octets, with no name to resolve.
    ///
    /// Builds the same sixteen-byte `struct sockaddr_in`, in the same
    /// Linux layout, that `examples/fetch/`'s `connect_to` builds by hand
    /// (`docs/connect.md` §3) — this is that code, once, in the backend,
    /// so a program with `Net` gets it instead of writing it again. `args`
    /// is the capability (zero-sized, stopping here), then the four
    /// octets, then the port, each an `int`.
    pub(crate) fn connect(&mut self, args: &[Expr]) -> Vec<Value> {
        let pointer = self.pointer;
        let octets: Vec<Value> = args[1..5].iter().map(|a| self.scalar(a)).collect();
        let port = self.scalar(&args[5]);

        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            16,
            0,
        ));
        let addr = self.builder.ins().stack_addr(pointer, slot, 0);

        // `sa_family_t sin_family`, little-endian: `2, 0` on Linux. macOS
        // reads family 0 with `sin_len` taken from the call's length
        // argument, which is why these same two bytes connect on both
        // targets (`docs/connect.md` §3).
        let zero8 = self.builder.ins().iconst(types::I8, 0);
        let family = self.builder.ins().iconst(types::I8, 2);
        self.builder.ins().store(MemFlags::trusted(), family, addr, 0);
        self.builder.ins().store(MemFlags::trusted(), zero8, addr, 1);

        // The port, big-endian.
        let port32 = self.builder.ins().ireduce(types::I32, port);
        let high = self.builder.ins().ushr_imm(port32, 8);
        let high = self.builder.ins().ireduce(types::I8, high);
        let low = self.builder.ins().ireduce(types::I8, port32);
        self.builder.ins().store(MemFlags::trusted(), high, addr, 2);
        self.builder.ins().store(MemFlags::trusted(), low, addr, 3);

        // The four octets, then eight bytes of padding.
        for (i, octet) in octets.iter().enumerate() {
            let byte = self.builder.ins().ireduce(types::I8, *octet);
            self.builder.ins().store(MemFlags::trusted(), byte, addr, 4 + i as i32);
        }
        for i in 0..8i32 {
            self.builder.ins().store(MemFlags::trusted(), zero8, addr, 8 + i);
        }

        let socket = self.libc_fn("socket", &[types::I32, types::I32, types::I32], &[types::I32]);
        let socket = self.module.declare_func_in_func(socket, self.builder.func);
        // `AF_INET`, `SOCK_STREAM`, the default protocol.
        let domain = self.builder.ins().iconst(types::I32, 2);
        let kind = self.builder.ins().iconst(types::I32, 1);
        let proto = self.builder.ins().iconst(types::I32, 0);
        let call = self.builder.ins().call(socket, &[domain, kind, proto]);
        let fd = self.builder.inst_results(call)[0];
        // Dominates both the failure jump below and the successful-connect
        // one further down, so it can be reused in either.
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
        let connect = self.libc_fn("connect", &[types::I32, pointer, types::I32], &[types::I32]);
        let connect = self.module.declare_func_in_func(connect, self.builder.func);
        let len = self.builder.ins().iconst(types::I32, 16);
        let call = self.builder.ins().call(connect, &[fd, addr, len]);
        let result = self.builder.inst_results(call)[0];

        let connected = self.builder.create_block();
        let not_connected = self.builder.create_block();
        let ok = self.builder.ins().icmp_imm(IntCC::Equal, result, 0);
        self.builder.ins().brif(ok, connected, &[], not_connected, &[]);

        self.builder.switch_to_block(connected);
        self.builder.seal_block(connected);
        let fd64 = self.builder.ins().sextend(types::I64, fd);
        self.builder.ins().jump(merge, &[fd64.into()]);

        self.builder.switch_to_block(not_connected);
        self.builder.seal_block(not_connected);
        let close = self.libc_fn("close", &[types::I32], &[types::I32]);
        let close = self.module.declare_func_in_func(close, self.builder.func);
        self.builder.ins().call(close, &[fd]);
        self.builder.ins().jump(merge, &[minus_one.into()]);

        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        vec![self.builder.block_params(merge)[0]]
    }
}
