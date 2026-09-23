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

    /// `connect(net, name, port)` — `docs/net.md` §4.1: a name and a port,
    /// checked against the capability's bound and only then resolved
    /// (`docs/connect.md` §10).
    ///
    /// Checked here rather than through a written signature because the
    /// row it performs is the bound its `Net` capability was narrowed to,
    /// exactly the reason [`Self::file_op`] is checked here and not there.
    pub(crate) fn connect(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, name, port] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`connect` takes 3 arguments -- the capability, the name and a port -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        let capability_span = self.ast.expr_span(*capability);
        let (net_value, net_ty) = self.expr(*capability)?;
        let bound = self.granted_net_bound(&net_ty, capability_span)?;

        let bytes = Type::Ref {
            unique: false,
            region: self.unifier.fresh_region(),
            inner: Box::new(Type::Slice(Box::new(Type::Byte))),
        };
        let name_span = self.ast.expr_span(*name);
        let (name_value, name_ty) = self.expr(*name)?;
        self.expect_type(&bytes, &name_ty, name_span)?;

        let port_span = self.ast.expr_span(*port);
        let (port_value, port_ty) = self.expr(*port)?;
        self.expect_type(&Type::Int, &port_ty, port_span)?;

        self.performed.union(&Effects::new([Label {
            name: "net_out".to_owned(),
            argument: Some(bound.clone()),
        }]));

        Ok((Expr::Connect { bound, args: vec![net_value, name_value, port_value] }, Type::Int))
    }

    /// `bind(net, port)` — `docs/net.md` §2.1: the inbound half, bound by
    /// *which port* alone, not `host:port` (`docs/listen.md` §6.1).
    ///
    /// Checked here for the same reason [`Self::connect`] is: the row is
    /// the bound the capability was narrowed to.
    pub(crate) fn bind(&mut self, args: &[ExprId], span: Span) -> Result<(Expr, Type), Diagnostic> {
        let [capability, port] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`bind` takes 2 arguments -- the capability and a port -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        let capability_span = self.ast.expr_span(*capability);
        let (net_value, net_ty) = self.expr(*capability)?;
        let bound = self.granted_net_bound(&net_ty, capability_span)?;

        let port_span = self.ast.expr_span(*port);
        let (port_value, port_ty) = self.expr(*port)?;
        self.expect_type(&Type::Int, &port_ty, port_span)?;

        self.performed.union(&Effects::new([Label {
            name: "net_in".to_owned(),
            argument: Some(bound.clone()),
        }]));

        Ok((Expr::Bind { bound, args: vec![net_value, port_value] }, Type::Int))
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
                "`{}` is not a borrowed `Net`; the network is reached through the capability that names what it may touch",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }
}
