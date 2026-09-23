//! Expressions: the one large dispatch over every expression form.

use crate::*;

impl<'a> FnLowering<'a> {
    /// A condition is a `bool`. M0 tested "non-zero"; M1 has a type for the
    /// question, so the convention becomes a rule.
    pub(crate) fn condition(&mut self, id: ExprId) -> Result<Expr, Diagnostic> {
        let (expr, found) = self.expr(id)?;
        self.expect_type(&Type::Bool, &found, self.ast.expr_span(id))?;
        Ok(expr)
    }

    pub(crate) fn expr(&mut self, id: ExprId) -> Result<(Expr, Type), Diagnostic> {
        let span = self.ast.expr_span(id);
        Ok(match self.ast.expr(id) {
            AstExpr::Int(v) => (Expr::Int(*v), Type::Int),
            AstExpr::Float(bits) => (Expr::Float(*bits), Type::Float),
            // A literal the checker reads and the program never holds.
            // `docs/strings.md` §4: the bytes go in the object file and the
            // slice points at them, so the region is `static` -- it outlives
            // everything, because the data is not in any frame.
            //
            // *Shared*, never unique: two occurrences of `"ok"` may be the
            // same bytes, and a program that could write through one would
            // be writing through both.
            //
            // `narrow` intercepts its own argument before it reaches here,
            // so this is every other place a literal can appear.
            AstExpr::Str(text) => (
                Expr::Bytes(text.clone()),
                Type::Ref {
                    unique: false,
                    region: Region::Static,
                    inner: Box::new(Type::Slice(Box::new(Type::Byte))),
                },
            ),
            AstExpr::Bool(v) => (Expr::Bool(*v), Type::Bool),
            AstExpr::Name(name) => {
                let text = self.ast.name_of(*name);
                match self.lookup(*name) {
                    Some(binding) => {
                        let (slot, ty) = (binding.slot, binding.ty.clone());
                        // Slice 1 has no borrowing, so every read of a `res`
                        // binding is a move. §5 adds the other kind.
                        self.trace.emit(Event::Use { slot, span });
                        (Expr::Load(slot), ty)
                    }
                    // `docs/compile-time-data.md` §2: the name reads as a
                    // `&static [T]`, exactly as a string literal does.
                    None if self.static_index(*name).is_some() => {
                        let index = self.static_index(*name).expect("just checked");
                        if let Some(current) = self.lowering_static {
                            if index >= current {
                                return Err(Diagnostic::new(
                                    Rule::StaticItem,
                                    format!(
                                        "`{text}` is a `static` declared later; a `static` may read one declared before it, so that there are no cycles to resolve"
                                    ),
                                    span,
                                ));
                            }
                        }
                        let referent = self.statics[index as usize].referent.clone();
                        (
                            Expr::Static(index),
                            Type::Ref {
                                unique: false,
                                region: Region::Static,
                                inner: Box::new(referent),
                            },
                        )
                    }
                    None if self.signatures.iter().any(|s| s.name == *name)
                        || Builtin::from_name(text).is_some() =>
                    {
                        return Err(Diagnostic::new(
                            Rule::NoFunctionValues,
                            format!(
                                "`{text}` is a function; M1 has no function values, so it can only be called"
                            ),
                            span,
                        ));
                    }
                    None => {
                        return Err(Diagnostic::new(
                            Rule::UnknownName,
                            format!("`{text}` is not bound here"),
                            span,
                        ));
                    }
                }
            }
            AstExpr::StructLit { name, qualifier, fields } => {
                let text = self.ast.name_of(*name);
                let target = self.target_module(*qualifier, span)?;
                // `docs/editions.md` §7: same "latest match this file's
                // edition allows" rule as `resolve_type_at`'s `lookup`.
                let Some(def) = self
                    .defs
                    .iter()
                    .filter(|d| {
                        d.name == *name && d.visible_from(target) && d.since <= self.edition
                    })
                    .max_by_key(|d| d.since)
                else {
                    return Err(Diagnostic::new(
                        Rule::NotAStruct,
                        format!("`{text}` is not a struct"),
                        span,
                    ));
                };
                if is_capability(def.def) {
                    return Err(Diagnostic::new(
                        Rule::CapabilityMisused,
                        format!(
                            "`{text}` is a capability and has no literal form; authority comes from the `World` the runtime hands `main`, and from nowhere else"
                        ),
                        span,
                    ));
                }
                let DefKind::Struct(fields_decl) = &def.kind else {
                    return Err(Diagnostic::new(
                        Rule::NotAStruct,
                        format!("`{text}` is an enum, not a struct"),
                        span,
                    ));
                };
                self.check_visible(target, def.public, "type", *name, span)?;
                // Copied out before checking any field value, because
                // checking borrows `self` and the table lives beside it.
                let (def_id, generic_count) = (def.def, def.generics.len());
                let fields_decl = fields_decl.clone();
                // A generic struct's arguments are inferred from the values
                // given for its fields, or left for the context to settle.
                let type_args = self.fresh_args(generic_count);
                let declared: Vec<(Symbol, Type)> =
                    fields_decl.iter().map(|(n, t)| (*n, t.substitute(&type_args, &[]))).collect();

                let mut values: Vec<Option<Expr>> = vec![None; declared.len()];
                let mut written_so_far: Option<usize> = None;
                for (field, value) in fields {
                    let field_text = self.ast.name_of(*field);
                    let Some(index) = declared.iter().position(|(n, _)| n == field) else {
                        return Err(Diagnostic::new(
                            Rule::UnknownName,
                            format!("`{text}` has no field `{field_text}`"),
                            span,
                        ));
                    };
                    if values[index].is_some() {
                        return Err(Diagnostic::new(
                            Rule::DuplicateDeclaration,
                            format!("field `{field_text}` is given twice"),
                            span,
                        ));
                    }
                    // The fields run in *declaration* order, because that is
                    // the order a struct's leaves are laid out in and the
                    // order the backend evaluates them. Writing them in some
                    // other order would mean side effects happening in an
                    // order the text does not show -- so it is refused
                    // rather than silently reordered
                    // (`docs/defined-behaviour.md`). The order you read is
                    // the order it runs.
                    if let Some(previous) = written_so_far
                        && index < previous
                    {
                        return Err(Diagnostic::new(
                            Rule::FieldOrder,
                            format!(
                                "field `{field_text}` is written after `{}`, but `{text}` declares it before; a struct literal's fields run in declaration order, so writing them in another order would hide what runs first",
                                self.ast.name_of(declared[previous].0)
                            ),
                            self.ast.expr_span(*value),
                        ));
                    }
                    written_so_far = Some(index);
                    let value_span = self.ast.expr_span(*value);
                    let (lowered, found) = self.expr(*value)?;
                    self.expect_type(&declared[index].1, &found, value_span)?;
                    values[index] = Some(lowered);
                }

                // A struct value has every field or it is not one. There is no
                // default to fall back on and no zero to invent.
                if let Some(missing) = values.iter().position(Option::is_none) {
                    return Err(Diagnostic::new(
                        Rule::MissingField,
                        format!(
                            "missing field `{}` in `{text}`",
                            self.ast.name_of(declared[missing].0)
                        ),
                        span,
                    ));
                }

                (
                    Expr::Struct {
                        def: def_id,
                        fields: values.into_iter().map(|v| v.expect("checked above")).collect(),
                    },
                    Type::Named(def_id, type_args),
                )
            }
            // `(a, b)` (`docs/tuples.md`). Components evaluate left to
            // right, which is not a choice made here -- it is
            // `defined-behaviour.md` §3, and lowering in order is how every
            // other argument list gets it.
            AstExpr::Tuple(parts) => {
                let parts = parts.clone();
                if parts.len() < 2 {
                    return Err(Diagnostic::new(
                        Rule::PatternShape,
                        "a tuple has two components or more; `(e)` is grouping, and neither `(e,)` nor `()` is a tuple",
                        span,
                    ));
                }
                let mut lowered = Vec::with_capacity(parts.len());
                let mut components = Vec::with_capacity(parts.len());
                for part in parts {
                    let (expr, ty) = self.expr(part)?;
                    lowered.push(expr);
                    components.push(ty);
                }
                (Expr::Tuple { parts: lowered }, Type::Tuple(components))
            }
            // `t.0` (§3.1). Every rule below is `p.x`'s rule with a number
            // in place of a name, which is the point: a tuple is an
            // anonymous struct, so it had better not need its own ideas
            // about reading a component.
            AstExpr::TupleField { base, index } => {
                let (base_id, index) = (*base, *index);
                let base_span = self.ast.expr_span(base_id);
                let (lowered, base_ty) = self.expr(base_id)?;
                let mut resolved = self.unifier.resolve(&base_ty);
                let through_reference = match &resolved {
                    Type::Ref { unique, region, .. } => Some((*unique, *region)),
                    _ => None,
                };
                if let Type::Ref { inner, .. } = resolved {
                    resolved = *inner;
                }
                let Type::Tuple(components) = resolved else {
                    return Err(Diagnostic::new(
                        Rule::NotATuple,
                        format!(
                            "`{}` is not a tuple, so it has no component `{index}`",
                            self.unifier.display(&resolved)
                        ),
                        base_span,
                    ));
                };
                let Some(ty) = components.get(index as usize).cloned() else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!(
                            "this tuple has {} components, so there is no `.{index}`; they are numbered from 0",
                            components.len()
                        ),
                        span,
                    ));
                };
                if let Some((unique, region)) = through_reference {
                    // `docs/reading-references.md` §2.0, in the place tuples
                    // add. A `val` component is copied out, which costs the
                    // referent nothing; a `res` one cannot be copied, so
                    // what comes back is a reference to it. A tuple is an
                    // anonymous struct, so it had better not need its own
                    // ideas about this either.
                    let base = Box::new(lowered);
                    if mode_of(self.defs, self.unifier, &self.bounds, &ty) == Mode::Res {
                        return Ok((
                            Expr::TupleFieldAddr { base, components, index },
                            Type::Ref { unique, region, inner: Box::new(ty) },
                        ));
                    }
                    return Ok((Expr::TupleFieldRef { base, components, index }, ty));
                }
                self.trace
                    .emit(Event::Read { ty: Type::Tuple(components.clone()), span: base_span });
                (Expr::TupleField { base: Box::new(lowered), components, index }, ty)
            }
            AstExpr::Field { base, name } => {
                let base_span = self.ast.expr_span(*base);
                let (lowered, base_ty) = self.expr(*base)?;
                let mut resolved = self.unifier.resolve(&base_ty);
                // `r.x` where `r : &r Point` reads through the reference.
                // One level: a reference to a reference has to be written
                // through twice, because auto-dereferencing a chain is the
                // kind of convenience that makes a cost invisible.
                // The base's mode and region travel with it: a field of a
                // `&!r` is reachable uniquely and a field of a `&r` is not,
                // and either way the field's reference lives exactly as long
                // as the one it was reached through (§2.0).
                let through_reference = match &resolved {
                    Type::Ref { unique, region, .. } => Some((*unique, *region)),
                    _ => None,
                };
                if let Type::Ref { inner, .. } = resolved {
                    resolved = *inner;
                }
                let Type::Named(def_id, type_args) = resolved else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{}` has no fields", self.unifier.display(&resolved)),
                        base_span,
                    ));
                };
                let def = self
                    .defs
                    .iter()
                    .find(|d| d.def == def_id)
                    .expect("a named type is a declared type");
                let field_text = self.ast.name_of(*name);
                let DefKind::Struct(fields) = &def.kind else {
                    return Err(Diagnostic::new(
                        Rule::MatchOnANonEnum,
                        format!(
                            "`{}` is an enum; its payload is read by matching on it, not with `.`",
                            self.ast.name_of(def.name)
                        ),
                        base_span,
                    ));
                };
                let Some(index) = fields.iter().position(|(n, _)| n == name) else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{}` has no field `{field_text}`", self.ast.name_of(def.name)),
                        span,
                    ));
                };
                let ty = fields[index].1.substitute(&type_args, &[]);
                if let Some((unique, region)) = through_reference {
                    // `docs/reading-references.md` §2: **nothing moves out
                    // of a reference, ever.** That rule was stated for
                    // `match`, and field access is the other way to reach
                    // into a value, so it holds here too.
                    //
                    // It decides what comes back rather than whether
                    // anything does. A `val` field is *copied*, which costs
                    // the referent nothing and is what a reference is for.
                    // A `res` field cannot be copied -- that is what `res`
                    // means -- so what comes back is a **reference to** it,
                    // exactly as `match` on a reference binds a `res`
                    // payload. Nothing moves either way, and the double
                    // free that copying one caused is unexpressible either
                    // way (§2.0).
                    //
                    // Reading through a reference is what a reference is
                    // *for*, so no `Read` event: the referent is frozen for
                    // the whole region and nothing is being moved.
                    let base = Box::new(lowered);
                    if mode_of(self.defs, self.unifier, &self.bounds, &ty) == Mode::Res {
                        return Ok((
                            Expr::FieldAddr {
                                base,
                                def: def_id,
                                args: type_args,
                                index: index as u32,
                            },
                            Type::Ref { unique, region, inner: Box::new(ty) },
                        ));
                    }
                    return Ok((
                        Expr::FieldRef { base, def: def_id, args: type_args, index: index as u32 },
                        ty,
                    ));
                }
                self.trace.emit(Event::Read {
                    ty: Type::Named(def_id, type_args.clone()),
                    span: base_span,
                });
                (
                    Expr::Field {
                        base: Box::new(lowered),
                        def: def_id,
                        args: type_args,
                        index: index as u32,
                    },
                    ty,
                )
            }
            AstExpr::Variant { enum_name, qualifier, variant, args } => {
                let enum_text = self.ast.name_of(*enum_name);
                let variant_text = self.ast.name_of(*variant);
                let target = self.target_module(*qualifier, span)?;
                // `docs/editions.md` §7: same "latest match this file's
                // edition allows" rule as `resolve_type_at`'s `lookup`.
                let Some(def) = self
                    .defs
                    .iter()
                    .filter(|d| {
                        d.name == *enum_name && d.visible_from(target) && d.since <= self.edition
                    })
                    .max_by_key(|d| d.since)
                else {
                    return Err(Diagnostic::new(
                        Rule::NotAnEnum,
                        format!("`{enum_text}` is not an enum"),
                        span,
                    ));
                };
                let public = def.public;
                let DefKind::Enum(variants) = &def.kind else {
                    return Err(Diagnostic::new(
                        Rule::NotAnEnum,
                        format!("`{enum_text}` is a struct, not an enum"),
                        span,
                    ));
                };
                let Some(index) = variants.iter().position(|(n, _)| n == variant) else {
                    return Err(Diagnostic::new(
                        Rule::UnknownName,
                        format!("`{enum_text}` has no variant `{variant_text}`"),
                        span,
                    ));
                };
                let (def_id, generic_count) = (def.def, def.generics.len());
                let declared_payload = variants[index].1.clone();
                self.check_visible(target, public, "type", *enum_name, span)?;
                // Inferred from the payload values, or left for the context:
                // `Option::None` learns its `T` from where it is used.
                let type_args = self.fresh_args(generic_count);
                let payload_types: Vec<Type> =
                    declared_payload.iter().map(|t| t.substitute(&type_args, &[])).collect();

                if args.len() != payload_types.len() {
                    return Err(Diagnostic::new(
                        Rule::ArityMismatch,
                        format!(
                            "`{enum_text}::{variant_text}` carries {} value{}, but {} {} given",
                            payload_types.len(),
                            if payload_types.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut payload = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(payload_types.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    payload.push(value);
                }

                (
                    Expr::Enum {
                        def: def_id,
                        args: type_args.clone(),
                        variant: index as u32,
                        payload,
                    },
                    Type::Named(def_id, type_args),
                )
            }
            AstExpr::Unary { op, operand } => {
                let operand_span = self.ast.expr_span(*operand);
                let (inner, found) = self.expr(*operand)?;
                match op {
                    ast::UnOp::Neg => {
                        if !matches!(self.unifier.resolve(&found), Type::Int | Type::Float) {
                            return Err(Diagnostic::new(
                                Rule::OperatorTypeMismatch,
                                format!(
                                    "`{}` cannot be negated (`int` and `float` can)",
                                    self.unifier.display(&found)
                                ),
                                operand_span,
                            ));
                        }
                        let ty = self.unifier.resolve(&found);
                        let folded = fold::negate(&inner, &ty);
                        let node = self.settle_fold(folded, Expr::Neg(Box::new(inner)), span)?;
                        (node, ty)
                    }
                    ast::UnOp::Not => {
                        self.expect_type(&Type::Bool, &found, operand_span)?;
                        let folded = fold::not(&inner);
                        (self.settle_fold(folded, Expr::Not(Box::new(inner)), span)?, Type::Bool)
                    }
                    ast::UnOp::BitNot => {
                        self.expect_type(&Type::Int, &found, operand_span)?;
                        let folded = fold::bit_not(&inner);
                        (self.settle_fold(folded, Expr::BitNot(Box::new(inner)), span)?, Type::Int)
                    }
                    ast::UnOp::Deref => self.deref(inner, &found, operand_span)?,
                }
            }
            AstExpr::Binary { op, lhs, rhs } => {
                let op = bin_op(*op);
                let lhs_span = self.ast.expr_span(*lhs);
                let rhs_span = self.ast.expr_span(*rhs);
                let (l, lt) = self.expr(*lhs)?;
                // `&&` and `||` do not evaluate their right operand when the
                // left already decides, so anything it consumes is consumed
                // conditionally — the same join as an `if` with no `else`.
                let short_circuit = op.is_short_circuit();
                if short_circuit {
                    self.trace.open();
                }
                let (r, rt) = self.expr(*rhs)?;
                if short_circuit {
                    let events = self.trace.close();
                    self.trace.emit(Event::Branch { arms: vec![events, Vec::new()], span });
                }

                // Both sides agree first, then the operator says what it
                // accepts. Reporting in that order blames the operand that
                // disagrees rather than the operator.
                self.expect_type(&lt, &rt, rhs_span)?;
                let operand = self.unifier.resolve(&lt);
                match op {
                    // `+ - * /` are `int` or `float`; `%` is `int` only.
                    // There is no `frem` primitive worth the name -- C's
                    // `fmod` is a library call with its own rounding
                    // story -- so it belongs in `std.math` when floats
                    // get one (`docs/floating-point.md` §7).
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        if !matches!(operand, Type::Int | Type::Float) {
                            return Err(Diagnostic::new(
                                Rule::OperatorTypeMismatch,
                                format!(
                                    "`{}` has no arithmetic (`int` and `float` do)",
                                    self.unifier.display(&operand)
                                ),
                                lhs_span,
                            ));
                        }
                    }
                    BinOp::Rem => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    BinOp::And | BinOp::Or => {
                        self.expect_type(&Type::Bool, &operand, lhs_span)?;
                    }
                    // `docs/floating-point.md` §5: IEEE's comparisons,
                    // which means NaN is unordered against everything and
                    // trichotomy fails. That is inherited rather than
                    // chosen, and `is_nan` exists so it is checkable.
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if !matches!(operand, Type::Int | Type::Float) {
                            return Err(Diagnostic::new(
                                Rule::OperatorTypeMismatch,
                                format!(
                                    "`{}` has no ordering (`int` and `float` do)",
                                    self.unifier.display(&operand)
                                ),
                                lhs_span,
                            ));
                        }
                    }
                    // `int` and nothing else. A `bool` is refused here
                    // rather than treated as one bit, because `&` and `&&`
                    // would then differ only in whether they short-circuit
                    // and a typo would compile (`docs/bitwise.md` §1). A
                    // `byte` is refused for `strings.md` §2's reason: it
                    // has no arithmetic, and masking is arithmetic.
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        self.expect_type(&Type::Int, &operand, lhs_span)?;
                    }
                    // `==` and `!=` compare two values of the same *scalar*
                    // type. Structs would need a field-wise comparison, which
                    // is a decision about what equality means rather than a
                    // missing instruction, so M1 refuses instead of guessing.
                    //
                    // `byte` is among them: comparing storage is not
                    // arithmetic (`docs/strings.md` §2), and a parser that
                    // cannot say `b == byte_of(44)` is not worth having.
                    BinOp::Eq | BinOp::Ne => {
                        if !matches!(operand, Type::Int | Type::Bool | Type::Byte | Type::Float) {
                            return Err(Diagnostic::new(
                                Rule::OperatorTypeMismatch,
                                format!(
                                    "`{}` cannot be compared with `==` (`int`, `byte`, `bool` and `float` can)",
                                    self.unifier.display(&operand)
                                ),
                                lhs_span,
                            ));
                        }
                    }
                }
                let result = op.result(&operand);
                // `docs/compile-time.md` §2.1: the backend cannot fold
                // this, because a checked add is `sadd_overflow` plus a
                // `trapnz` and the egraph's rules are written for the
                // plain form. Here the literals are in hand, so whether
                // it traps is a question with an answer.
                let folded = fold::bin(op, &l, &r);
                let node = Expr::Bin { op, lhs: Box::new(l), rhs: Box::new(r) };
                (self.settle_fold(folded, node, span)?, result)
            }
            AstExpr::Alloc { region, value } => return self.alloc(*region, *value, span),
            AstExpr::AllocSlice { region, count, fill } => {
                return self.alloc_slice(*region, *count, *fill, span);
            }
            AstExpr::Index { base, index } => return self.index(*base, *index, span),
            AstExpr::Slice { base, start, end } => {
                return self.subslice(*base, *start, *end, span);
            }
            AstExpr::Call { callee, qualifier, args } => {
                let text = self.ast.name_of(*callee);
                if self.lookup(*callee).is_some() {
                    return Err(Diagnostic::new(
                        Rule::NotAFunction,
                        format!("`{text}` is a local binding, not a function"),
                        span,
                    ));
                }

                // A generic callee is instantiated with fresh variables, which
                // the argument types then solve. The signature is all a caller
                // is ever checked against, generic or not.
                let mut instantiate: Option<(usize, Vec<Type>)> = None;
                let mut region_args: Vec<Region> = Vec::new();
                let mut foreign: Option<u32> = None;
                // §7.2: a body's row is a union over its calls, taken from
                // **the callee this call actually resolves to**.
                //
                // This used to be its own lookup: module-blind, and
                // consulting `signatures` before `externs` where the type
                // resolution below does the opposite. Two walks that could
                // disagree, which the comment here claimed there was not
                // one of -- and once `docs/modules.md` let two modules hold
                // one name, they did. A root `extern fn puts` called from
                // the root took its *types* from the extern and its
                // *effects* from an unrelated `puts` in another module: a
                // function calling into C, declaring `[]`, and compiling.
                //
                // So it resolves once, here, and everything below reads the
                // answer.
                let target = self.target_module(*qualifier, span)?;
                let resolved = Resolved::find(self, text, *callee, target);
                self.performed.union(&resolved.effects(self));
                // §7.4: narrowing is checked here because both its argument
                // and its result depend on the literal that was written.
                if Builtin::from_name(text) == Some(Builtin::Narrow) {
                    return self.narrow(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Release) {
                    return self.release(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Len) {
                    return self.len(args, span);
                }
                if let Some(op @ (Builtin::FsRead | Builtin::FsWrite)) = Builtin::from_name(text) {
                    return self.file_op(op, args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::OpenRead) {
                    return self.open_read(args, span);
                }
                // `docs/heap.md` §3: all three depend on the type being
                // boxed, which no fixed signature has a parameter to name.
                if Builtin::from_name(text) == Some(Builtin::Box) {
                    return self.boxed(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Unbox) {
                    return self.unboxed(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::Contents) {
                    return self.contents(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::BoxSlice) {
                    return self.boxed_slice(args, span);
                }
                if Builtin::from_name(text) == Some(Builtin::UnboxSlice) {
                    return self.unboxed_slice(args, span);
                }
                // `docs/editions.md` §7: `split`'s return type depends on
                // the caller's edition, which a fixed signature cannot
                // express. `resolved` rather than `Builtin::from_name`
                // guards this and `connect` below, because only `resolved`
                // has already been filtered by edition (`Resolved::find`)
                // -- an edition-1 file's own `extern fn connect` must reach
                // its `Extern` arm, not this one.
                if resolved == Resolved::Builtin(Builtin::Split) {
                    return self.split(args, span);
                }
                if resolved == Resolved::Builtin(Builtin::Connect) {
                    return self.connect(args, span);
                }
                if resolved == Resolved::Builtin(Builtin::Bind) {
                    return self.bind(args, span);
                }
                let (params, ret) = if let Resolved::Builtin(builtin) = resolved {
                    // A builtin's region parameters are instantiated exactly
                    // like a written function's (§5.1): one fresh region per
                    // parameter, solved by the arguments.
                    let fresh: Vec<Region> =
                        (0..builtin.regions()).map(|_| self.unifier.fresh_region()).collect();
                    let prelude = self.prelude();
                    let (params, ret) = builtin.signature(&prelude);
                    (
                        params.iter().map(|t| t.substitute(&[], &fresh)).collect::<Vec<_>>(),
                        ret.substitute(&[], &fresh),
                    )
                } else if let Resolved::Extern(index) = resolved {
                    // A foreign call is checked against its declaration and
                    // nothing else, exactly like a written function's (§8.4).
                    // Its region parameters are instantiated here too, since
                    // the capability it takes is borrowed.
                    let ext = &self.externs[index];
                    let (params, ret) = (ext.params.clone(), ext.ret.clone());
                    let count = params
                        .iter()
                        .filter_map(|t| match t {
                            Type::Ref { region: Region::Param(i), .. } => Some(*i + 1),
                            _ => None,
                        })
                        .max()
                        .unwrap_or(0) as usize;
                    let fresh: Vec<Region> =
                        (0..count).map(|_| self.unifier.fresh_region()).collect();
                    foreign = Some(index as u32);
                    (
                        params.iter().map(|t| t.substitute(&[], &fresh)).collect::<Vec<_>>(),
                        ret.substitute(&[], &fresh),
                    )
                } else {
                    let Resolved::Fn(index) = resolved else {
                        return Err(Diagnostic::new(
                            Rule::NotAFunction,
                            format!("`{text}` is not a function in this program"),
                            span,
                        ));
                    };
                    self.check_visible(
                        target,
                        self.signatures[index].public,
                        "function",
                        *callee,
                        span,
                    )?;
                    let fresh = self.fresh_args(self.signatures[index].generics.len());
                    // §5.1: each region parameter gets a variable the
                    // argument types then solve. One name, one assignment.
                    let fresh_regions: Vec<Region> = (0..self.signatures[index].regions.len())
                        .map(|_| self.unifier.fresh_region())
                        .collect();
                    let signature = &self.signatures[index];
                    let params: Vec<Type> = signature
                        .params
                        .iter()
                        .map(|t| t.substitute(&fresh, &fresh_regions))
                        .collect();
                    let ret = signature.ret.substitute(&fresh, &fresh_regions);
                    instantiate = Some((index, fresh));
                    region_args = fresh_regions;
                    (params, ret)
                };

                if args.len() != params.len() {
                    return Err(Diagnostic::new(
                        Rule::ArityMismatch,
                        format!(
                            "`{text}` takes {} argument{}, but {} {} given",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            args.len(),
                            if args.len() == 1 { "was" } else { "were" }
                        ),
                        span,
                    ));
                }

                let mut lowered = Vec::with_capacity(args.len());
                for (&arg, expected) in args.iter().zip(params.iter()) {
                    let arg_span = self.ast.expr_span(arg);
                    let (value, found) = self.expr(arg)?;
                    self.expect_type(expected, &found, arg_span)?;
                    lowered.push(value);
                }
                // Every `where a <= b` the callee declared must hold between
                // the regions it was instantiated at. Same lexical lookup the
                // body used, so a caller discharges the obligation with the
                // same walk up the same stack (§5.2).
                if let Some((index, _)) = instantiate {
                    let obligations = self.signatures[index].outlives.clone();
                    let names = self.signatures[index].regions.clone();
                    for (inner, outer) in obligations {
                        let got_inner = self.unifier.resolve_region(region_args[inner as usize]);
                        let got_outer = self.unifier.resolve_region(region_args[outer as usize]);
                        if !self.outlives(got_outer, got_inner) {
                            return Err(Diagnostic::new(
                                Rule::ReferenceEscapesRegion,
                                format!(
                                    "`{text}` requires `{} <= {}`, but here `{}` does not outlive `{}`",
                                    self.ast.name_of(names[inner as usize]),
                                    self.ast.name_of(names[outer as usize]),
                                    self.unifier.display_region(got_outer),
                                    self.unifier.display_region(got_inner)
                                ),
                                span,
                            ));
                        }
                    }
                }

                let callee_ref = match instantiate {
                    None if foreign.is_some() => {
                        Callee::Extern(foreign.expect("checked by the guard"))
                    }
                    None => Callee::Builtin(
                        Builtin::from_name(text).expect("only a builtin skips instantiation"),
                    ),
                    Some((index, fresh)) => {
                        // Every type argument must be settled by the arguments.
                        // Letting the surrounding context settle one would mean
                        // deciding which copy to emit after the call was already
                        // lowered.
                        let mut settled = Vec::with_capacity(fresh.len());
                        for (position, var) in fresh.iter().enumerate() {
                            let resolved = self.unifier.resolve(var);
                            if resolved.is_var() {
                                let parameter =
                                    self.ast.name_of(self.signatures[index].generics[position]);
                                return Err(Diagnostic::new(
                                    Rule::AmbiguousType,
                                    format!(
                                        "cannot tell what `{parameter}` is in this call to `{text}`; it is not determined by the arguments"
                                    ),
                                    span,
                                ));
                            }
                            // `docs/mode-polymorphism.md` §3.1: a `[T: val]`
                            // parameter promises the callee only works for
                            // copyable types, and this is where the promise
                            // is kept -- at the **call site**, which is the
                            // half of §12 that was missing. Without it the
                            // callee's body would be refused instead, inside
                            // a library the caller cannot change.
                            if self.signatures[index].bounds.get(position).copied().flatten()
                                == Some(Mode::Val)
                                && mode_of(self.defs, self.unifier, &self.bounds, &resolved)
                                    == Mode::Res
                            {
                                let parameter =
                                    self.ast.name_of(self.signatures[index].generics[position]);
                                return Err(Diagnostic::new(
                                    Rule::ModeBoundViolated,
                                    format!(
                                        "`{text}` needs `{parameter}` to be `val`, and `{}` is `res`",
                                        self.unifier.display(&resolved)
                                    ),
                                    span,
                                ));
                            }
                            settled.push(resolved);
                        }
                        Callee::Fn(self.mono.request(index, settled))
                    }
                };
                (Expr::Call { callee: callee_ref, args: lowered }, ret)
            }
        })
    }
}
