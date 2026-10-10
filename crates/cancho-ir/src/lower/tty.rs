//! `tty_open`'s lowering (`docs/tty.md` §3): the capability's prefix
//! travels with the call, checked against the path at run time the way
//! `open_file`'s does, and the row records it (`§4`).

use crate::*;

impl FnLowering<'_> {
    /// `tty_open(tty, path) -> [tty_open(prefix)] TtyOpened` — the
    /// capability names a device-path prefix; the port is opened under it,
    /// nonblocking, no controlling terminal (`docs/tty.md` §3).
    pub(crate) fn tty_open(
        &mut self,
        args: &[ExprId],
        span: Span,
    ) -> Result<(Expr, Type), Diagnostic> {
        let [capability, path] = args else {
            return Err(Diagnostic::new(
                Rule::ArityMismatch,
                format!(
                    "`tty_open` takes 2 arguments -- the capability and the path -- but {} were given",
                    args.len()
                ),
                span,
            ));
        };
        let capability_span = self.ast.expr_span(*capability);
        let (tty_value, tty_ty) = self.expr(*capability)?;
        let prefix = self.granted_tty_prefix(&tty_ty, capability_span)?;
        let bytes = Type::Ref {
            unique: false,
            region: self.unifier.fresh_region(),
            inner: Box::new(Type::Slice(Box::new(Type::Byte))),
        };
        let path_span = self.ast.expr_span(*path);
        let (path_value, path_ty) = self.expr(*path)?;
        self.expect_type(&bytes, &path_ty, path_span)?;
        // `docs/tty.md` §4: the row names the prefix, so the report can say
        // which devices a program may reach — the sentence cancho-robot was
        // founded to make.
        self.performed.union(&Effects::new([Label {
            name: "tty_open".to_owned(),
            argument: Some(prefix.clone()),
        }]));
        Ok((
            Expr::TtyOpen { prefix, args: vec![tty_value, path_value] },
            Type::Named(self.prelude()[PRELUDE_TTY_OPENED], Vec::new()),
        ))
    }

    /// The prefix a borrowed `Tty` was narrowed to (`docs/tty.md` §4):
    /// `Fs`'s shape, refused with its own message when the argument is
    /// not a borrowed `Tty`.
    pub(crate) fn granted_tty_prefix(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Result<String, Diagnostic> {
        let resolved = self.unifier.resolve(ty);
        if let Type::Ref { inner, .. } = &resolved
            && let Type::Named(def, args) = self.unifier.resolve(inner)
            && def.0 as usize == PRELUDE_TTY
            && let Some(Type::Lit(prefix)) = args.first()
        {
            return Ok(prefix.clone());
        }
        Err(Diagnostic::new(
            Rule::CapabilityMisused,
            format!(
                "`{}` is not a borrowed `Tty`; a serial port is reached through the capability that names the device it may touch",
                self.unifier.display(&resolved)
            ),
            span,
        ))
    }
}
