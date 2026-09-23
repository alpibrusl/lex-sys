//! `split`'s edition-dependent return type, and `connect`
//! (`docs/net.md`, `docs/editions.md` §7).

use crate::*;

impl<'a> FnLowering<'a> {
    /// `split(w: World) -> Split` — consumes the root of all authority and
    /// hands back its capabilities (§8.2).
    ///
    /// Checked here rather than through [`Builtin::signature`] because the
    /// answer depends on the caller's edition (`docs/editions.md` §7): an
    /// edition-1 file gets the five-field `Split` it always has, and an
    /// edition-2 file gets the six-field one that also carries `Net`. A
    /// fixed signature has no way to make a return type depend on which
    /// file is calling.
    pub(crate) fn split(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [world] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!("`split` takes 1 argument, but {} were given", args.len()),
                span,
            ));
        };
        let world_span = self.ast.expr_span(*world);
        let (value, found) = self.expr(*world)?;
        let expected = Type::Named(self.prelude()[PRELUDE_WORLD], Vec::new());
        self.expect_type(&expected, &found, world_span)?;
        let split_index = if self.edition >= 2 { PRELUDE_SPLIT_NET } else { PRELUDE_SPLIT };
        Ok((
            Expr::Call { callee: Callee::Builtin(Builtin::Split), args: vec![value] },
            Type::Named(self.prelude()[split_index], Vec::new()),
        ))
    }

    /// `connect(net, a, b, c, d, port)` — `docs/net.md` §4.1, slice 1 of
    /// `Net` (`docs/connect.md` §1): the address is four octets a caller
    /// already has, the same shape `examples/fetch/`'s `connect_to` builds
    /// by hand today, with no name to resolve yet.
    ///
    /// Checked here rather than through a written signature because the
    /// row it performs is the bound its `Net` capability was narrowed to,
    /// exactly the reason [`Self::file_op`] is checked here and not there.
    pub(crate) fn connect(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, a, b, c, d, port] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`connect` takes 6 arguments -- the capability, four octets and a port -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        let capability_span = self.ast.expr_span(*capability);
        let (net_value, net_ty) = self.expr(*capability)?;
        let bound = self.granted_net_bound(&net_ty, capability_span)?;

        let mut lowered = vec![net_value];
        for octet in [*a, *b, *c, *d, *port] {
            let octet_span = self.ast.expr_span(octet);
            let (value, found) = self.expr(octet)?;
            self.expect_type(&Type::Int, &found, octet_span)?;
            lowered.push(value);
        }

        self.performed.union(&Effects::new([Label {
            name: "net_out".to_owned(),
            argument: Some(bound.clone()),
        }]));

        Ok((Expr::Connect { bound, args: lowered }, Type::Int))
    }

    /// The `host:port` bound a borrowed `Net` was narrowed to, the same
    /// shape [`Self::granted_prefix`] reads off a borrowed `Fs`.
    pub(crate) fn granted_net_bound(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let resolved = self.unifier.resolve(ty);
        if let Type::Ref { inner, .. } = &resolved
            && let Type::Named(def, args) = self.unifier.resolve(inner)
            && def.0 as usize == PRELUDE_NET
            && let Some(Type::Lit(bound)) = args.first()
        {
            return Ok(bound.clone());
        }
        Err(Diagnostic::new(
            Rule::CapabilityMisused,
            format!(
                "`{}` is not a borrowed `Net`; connecting is reached through the capability that names the host it may reach",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }
}
