//! Modes and linearity — `docs/linearity-and-effects.md` §3 and §4.
//!
//! Two rules, kept in one file because they are one idea:
//!
//! - **§3, modes.** Every type is `val` (unrestricted) or `res` (linear).
//!   Mode is structural: an aggregate is `res` if any member is. It is never
//!   inferred from use.
//! - **§4, linearity.** A `res` value is consumed *exactly* once on every
//!   path. Not affine — a value that reaches the end of its scope unconsumed
//!   is an error, because the case affine drops silently is the one the
//!   system exists to prevent. And no implicit destructors, because a
//!   destructor is code nobody wrote at a point nobody named.
//!
//! The checker runs over an **event trace** rather than over the IR. Lowering
//! records what happened — a binding entered scope, a slot was read, a branch
//! split, a loop closed — and this module replays it once the types are
//! settled. Two reasons that is the right shape:
//!
//! 1. A span is free while lowering and gone afterwards: the IR deliberately
//!    carries no spans, so a pass over it could not say *where*.
//! 2. A type may still be an inference variable at the moment it is used and
//!    only get settled by a later statement. Deciding a mode then would be
//!    guessing; deciding it at replay is reading.
//!
//! The replay is one walk with no fixpoint (§10): a branch is a join of its
//! arms, and a loop is an equality check at the back edge.

use lex_sys_syntax::{Diagnostic, Span};
use lex_sys_types::{Type, Unifier};

pub use lex_sys_syntax::ast::Mode;

use crate::{Slot, TypeDef};

/// The mode of a type, computed structurally.
///
/// `bounds` is the enclosing declaration's `val` bounds, one per type
/// parameter (`docs/mode-polymorphism.md` §3.1). An **unbounded**
/// parameter is `res`: the stronger obligation, so a body that satisfies
/// linearity for it satisfies linearity at every instantiation, and the
/// error for a generic that drops its parameter lands on the definition
/// rather than on whichever call site happened to use a resource type.
///
/// A monomorphised copy has no `Param` left at all, so `bounds` is empty
/// there and never consulted.
///
/// A generic *type* is another matter — `Pair[File]` is `res` and
/// `Pair[int]` is `val`, because the arguments are substituted in first.
pub(crate) fn mode_of(
    defs: &[TypeDef],
    unifier: &Unifier,
    bounds: &[Option<Mode>],
    ty: &Type,
) -> Mode {
    match unifier.resolve(ty) {
        Type::Named(def, args) => {
            let index = def.0 as usize;
            // A declared `res` is `res`, whatever it holds: the declaration
            // is asking for the stronger obligation and may have it.
            if defs[index].declared_mode == Some(Mode::Res) {
                return Mode::Res;
            }
            // A declared `val` is **not** believed here
            // (`docs/mode-polymorphism.md` §2). It was, and
            // `Wrap[Box[int]]` came out `val` by assertion -- a leak, and a
            // double free once copied. The members are walked either way,
            // and a `val` that computes `res` is a contradiction caught
            // where the promise was made: at the declaration for a
            // non-generic, and at the instantiation for a written generic
            // type. An *inferred* instantiation reaches neither, which is
            // why this has to be honest rather than trusting.
            // The type graph is acyclic — `collect_types` refused anything
            // else — so this recursion terminates without a seen set.
            let members: Vec<Type> = defs[index].members().cloned().collect();
            for member in members {
                if mode_of(defs, unifier, bounds, &member.substitute(&args, &[])) == Mode::Res {
                    return Mode::Res;
                }
            }
            Mode::Val
        }
        // `docs/tuples.md` §2.3: a tuple is `res` if any component is.
        //
        // The same structural rule as a struct's, arrived at differently: a
        // struct *declares* a mode and the declaration is checked against
        // the members, while a tuple has no declaration site, so there is
        // nothing to write and nothing to check. The mode is read off the
        // components instead, which is also how the mode of `Pair[File]`
        // has always been decided.
        Type::Tuple(parts) => {
            for part in &parts {
                if mode_of(defs, unifier, bounds, part) == Mode::Res {
                    return Mode::Res;
                }
            }
            Mode::Val
        }
        // `docs/mode-polymorphism.md` §3.1. `val` where the declaration
        // said so, `res` otherwise -- including for a parameter index the
        // caller did not supply a bound for, because assuming `res` is the
        // answer that is never unsound.
        Type::Param(i) => bounds.get(i as usize).copied().flatten().unwrap_or(Mode::Res),
        // §5 rule 3: `&r T` and `&!r T` are `val` whatever `T` is. Copyable
        // and discardable, which is sound precisely because the referent is
        // frozen or locked for the whole region and the region is a block.
        Type::Ref { .. } => Mode::Val,
        _ => Mode::Val,
    }
}

/// What lowering records for the checker to replay.
///
/// The shape is a tree rather than a flat stream with markers, because the
/// rules are about regions of the program — a scope's bindings, a branch's
/// arms, a loop's body — and a tree makes each region a value the checker can
/// run and compare.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// A binding entered scope. Its name and span are here so a diagnostic
    /// can point at the declaration rather than at the function.
    ///
    /// `shadows` is the binding of the same name this one replaced in the
    /// same block, if there was one (`docs/shadowing.md` §3). Lowering
    /// records the *link* and the checker reads the liveness, because
    /// whether a binding is dead is a fact about the trace and lowering
    /// cannot know it -- the same reason modes are decided here rather
    /// than where a type is first written.
    Declare { slot: Slot, name: String, span: Span, shadows: Option<Slot> },
    /// A slot was read. In M2 slice 1 there is no borrowing yet (§5), so
    /// every read of a `res` slot is a move.
    Use { slot: Slot, span: Span },
    /// A slot was assigned to. Overwriting a live `res` value would discard
    /// it, so the slot must be dead first.
    Assign { slot: Slot, span: Span },
    /// A value was produced and thrown away: an expression statement, a
    /// pattern position written `_`, or a `_` arm that never names the parts.
    /// Legal only for `val`.
    Discard { ty: Type, what: &'static str, span: Span },
    /// A part was read out of a value without taking the value apart. That is
    /// a borrow (§5), which M2 slice 1 does not have, so it is legal only for
    /// `val`.
    Read { ty: Type, span: Span },
    /// A `return`. Every live obligation must be discharged by now.
    Return { span: Span },
    /// A `borrow` block opened over this slot (§5 rules 1 and 2).
    ///
    /// Shared freezes: not movable, not consumable, not uniquely borrowable
    /// — a read is still fine, which is the whole point, and for a `val` slot
    /// a read was never a move so nothing changes.
    ///
    /// Unique *locks*: nothing else may touch it at all, not even a read.
    Freeze { slot: Slot, unique: bool, span: Span },
    /// The block closed and the slot is owned again. §5: "a three-valued flag
    /// set at block entry and restored at block exit".
    Thaw { slot: Slot, unique: bool },
    /// A block. Whatever it declared must be dead when it closes.
    Scope(Vec<Event>),
    /// `if`/`else`, a `match`'s arms, or the right-hand side of `&&`/`||`.
    /// Every arm that does not diverge must agree about what is live (§4.2).
    Branch { arms: Vec<Vec<Event>>, span: Span },
    /// A loop body, which must leave the live set exactly as it found it
    /// (§4.3). Not a fixpoint: the body is walked once and the sets compared.
    Loop { body: Vec<Event>, span: Span },
}

/// The open event sequences, innermost last.
///
/// Lowering pushes a sequence when it enters a region and pops it when it
/// leaves, so the tree is built by the same recursion that walks the AST.
#[derive(Default)]
pub(crate) struct Trace {
    stack: Vec<Vec<Event>>,
}

impl Trace {
    pub(crate) fn new() -> Self {
        Trace { stack: vec![Vec::new()] }
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.stack.last_mut().expect("a sequence is always open").push(event);
    }

    pub(crate) fn open(&mut self) {
        self.stack.push(Vec::new());
    }

    pub(crate) fn close(&mut self) -> Vec<Event> {
        self.stack.pop().expect("open and close are paired")
    }

    pub(crate) fn finish(mut self) -> Vec<Event> {
        self.stack.pop().expect("the outermost sequence is never closed")
    }
}

/// What the checker knows about one slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    /// A `val` slot, or one not yet declared: no obligation either way.
    Untracked,
    /// A `res` value is here and has not been consumed.
    Live,
    /// A `res` value was here and has been consumed.
    Moved,
}

struct Check<'a> {
    defs: &'a [TypeDef],
    unifier: &'a Unifier,
    /// The enclosing declaration's `val` bounds, for `Type::Param`
    /// (`docs/mode-polymorphism.md` §3.1).
    bounds: &'a [Option<Mode>],
    slots: &'a [Type],
    /// Filled in by `Declare`, so an error can name the binding it is about.
    names: Vec<Option<(String, Span)>>,
    state: Vec<State>,
    /// What each slot is borrowed as. `Owned | Frozen(n) | Locked` — §5's
    /// three-valued flag, with the shared case counting because shared
    /// borrows nest and the innermost closing must not thaw the whole thing.
    borrowed: Vec<Borrow>,
}

/// §5's three-valued flag: a binding is owned, frozen by some number of
/// shared borrows, or locked by exactly one unique borrow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Borrow {
    Owned,
    Frozen(u32),
    Locked,
}

/// Check one function body's trace. `slots` are the settled slot types.
pub(crate) fn check(
    defs: &[TypeDef],
    unifier: &Unifier,
    bounds: &[Option<Mode>],
    slots: &[Type],
    events: &[Event],
) -> Result<(), Diagnostic> {
    let mut check = Check {
        defs,
        unifier,
        bounds,
        slots,
        names: vec![None; slots.len()],
        state: vec![State::Untracked; slots.len()],
        borrowed: vec![Borrow::Owned; slots.len()],
    };
    check.run(events)?;
    Ok(())
}

impl Check<'_> {
    fn is_res(&self, slot: Slot) -> bool {
        mode_of(self.defs, self.unifier, self.bounds, &self.slots[slot.0 as usize]) == Mode::Res
    }

    fn name(&self, slot: Slot) -> String {
        match &self.names[slot.0 as usize] {
            Some((name, _)) => name.clone(),
            None => "a value".to_owned(),
        }
    }

    /// Replay a sequence, returning whether it ended on every path in a
    /// `return`.
    fn run(&mut self, events: &[Event]) -> Result<bool, Diagnostic> {
        let mut diverged = false;
        for event in events {
            match event {
                Event::Declare { slot, name, span, shadows } => {
                    // `docs/shadowing.md` §3: shadowing is allowed exactly
                    // when the shadowed binding is dead. That is the rule
                    // `Event::Assign` applies a few arms down, and it is
                    // the same rule for the same reason -- a live `res`
                    // value put out of reach is a leak, which is the case
                    // affine types drop silently.
                    if let Some(old) = shadows {
                        if self.state[old.0 as usize] == State::Live {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{name}` still holds a `res` value; shadowing it here would put that value out of reach with its obligation undischarged. Consume it first"
                                ),
                                *span,
                            ));
                        }
                    }
                    self.names[slot.0 as usize] = Some((name.clone(), *span));
                    self.state[slot.0 as usize] =
                        if self.is_res(*slot) { State::Live } else { State::Untracked };
                }
                Event::Freeze { slot, unique, span } => {
                    if self.state[slot.0 as usize] == State::Moved {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` has already been consumed; there is nothing left to borrow",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                    let name = self.name(*slot);
                    let held = &mut self.borrowed[slot.0 as usize];
                    *held = match (*held, unique) {
                        // §5 rule 2: one unique borrow at a time, and never
                        // alongside a shared one. A unique reference is the
                        // only way to reach the value, or it is not unique.
                        (Borrow::Locked, _) => {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{name}` is already uniquely borrowed; a `&!` reference is the only way to reach a value, so there is at most one"
                                ),
                                *span,
                            ));
                        }
                        (Borrow::Frozen(_), true) => {
                            return Err(Diagnostic::new(
                                format!(
                                    "`{name}` is already borrowed by an enclosing `borrow`, so it cannot be borrowed uniquely here"
                                ),
                                *span,
                            ));
                        }
                        (Borrow::Owned, true) => Borrow::Locked,
                        (Borrow::Owned, false) => Borrow::Frozen(1),
                        // Shared borrows nest: freezing is not exclusive.
                        (Borrow::Frozen(n), false) => Borrow::Frozen(n + 1),
                    };
                }
                Event::Thaw { slot, unique } => {
                    let held = &mut self.borrowed[slot.0 as usize];
                    *held = match (*held, unique) {
                        (Borrow::Frozen(n), false) if n > 1 => Borrow::Frozen(n - 1),
                        _ => Borrow::Owned,
                    };
                }
                // §5 rule 2: a locked binding may not be *touched* — not
                // read, not borrowed again, not moved. That is stronger than
                // frozen and it is what makes `&!` mean unique: if the owner
                // could still read it, the reference would not be the only
                // way to reach the value.
                Event::Use { slot, span } if self.borrowed[slot.0 as usize] == Borrow::Locked => {
                    return Err(Diagnostic::new(
                        format!(
                            "`{}` is uniquely borrowed here, so nothing else may read it; the reference is the only way to reach it",
                            self.name(*slot)
                        ),
                        *span,
                    ));
                }
                Event::Use { slot, span } => match self.state[slot.0 as usize] {
                    State::Untracked => {}
                    // Reading a `res` binding *is* moving it — there is no
                    // non-owning read of an owned name — so a frozen one
                    // cannot be read at all. §5 rule 1: frozen means not
                    // movable, not consumable.
                    State::Live if self.borrowed[slot.0 as usize] != Borrow::Owned => {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is frozen by an enclosing `borrow`, so it cannot be moved or consumed here",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                    State::Live => self.state[slot.0 as usize] = State::Moved,
                    State::Moved => {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` has already been consumed; a `res` value is used exactly once",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                },
                Event::Assign { slot, span } => {
                    // A frozen binding may not change underneath a reference
                    // to it. §5 rule 1 says not movable and not consumable;
                    // assignment is the third way to break the promise.
                    if self.borrowed[slot.0 as usize] != Borrow::Owned {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is borrowed by an enclosing `borrow`, so it cannot be assigned to here",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                    if self.state[slot.0 as usize] == State::Live {
                        return Err(Diagnostic::new(
                            format!(
                                "assigning to `{}` would discard the `res` value it still holds; consume it first",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                    if self.is_res(*slot) {
                        self.state[slot.0 as usize] = State::Live;
                    }
                }
                Event::Discard { ty, what, span } => {
                    if mode_of(self.defs, self.unifier, self.bounds, ty) == Mode::Res {
                        return Err(Diagnostic::new(
                            format!(
                                "{what} is `res` (`{}`), so it cannot be discarded; name the function that consumes it",
                                self.unifier.display(&self.unifier.resolve(ty))
                            ),
                            *span,
                        ));
                    }
                }
                Event::Read { ty, span } => {
                    if mode_of(self.defs, self.unifier, self.bounds, ty) == Mode::Res {
                        let resolved = self.unifier.resolve(ty);
                        // A tuple has components, not fields
                        // (`docs/tuples.md` §3.1). One rule, and it should
                        // say so in the vocabulary of whatever it is
                        // refusing.
                        let part = match resolved {
                            Type::Tuple(_) => "component",
                            _ => "field",
                        };
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is `res`, so a {part} cannot be read out of it; take the whole value apart with a destructuring `let` (a non-owning read is a borrow, which is §5)",
                                self.unifier.display(&resolved)
                            ),
                            *span,
                        ));
                    }
                }
                Event::Return { span } => {
                    if let Some(slot) = self.live_slot() {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is still live here; a `res` value must be consumed on every path",
                                self.name(slot)
                            ),
                            *span,
                        ));
                    }
                    diverged = true;
                }
                Event::Scope(body) => {
                    diverged = self.run(body)?;
                    if !diverged {
                        for event in body {
                            let Event::Declare { slot, .. } = event else { continue };
                            if self.state[slot.0 as usize] == State::Live {
                                let (name, span) =
                                    self.names[slot.0 as usize].clone().expect("just declared");
                                return Err(Diagnostic::new(
                                    format!(
                                        "`{name}` is still live at the end of this block; nothing consumes it"
                                    ),
                                    span,
                                ));
                            }
                        }
                    }
                }
                Event::Branch { arms, span } => diverged = self.branch(arms, *span)?,
                Event::Loop { body, span } => {
                    let before = self.state.clone();
                    let body_diverged = self.run(body)?;
                    self.forget_locals(&before);
                    if !body_diverged && self.state != before {
                        let slot = self.differing_slot(&before).expect("the states differ");
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is consumed inside this loop; the next iteration would use it again",
                                self.name(slot)
                            ),
                            *span,
                        ));
                    }
                    // A loop is never a terminator, matching `terminates`.
                    self.state = before;
                    diverged = false;
                }
            }
        }
        Ok(diverged)
    }

    /// Join the arms of a branch: every arm that can fall through must agree
    /// about what is live (§4.2). An arm that returns is not at the merge
    /// point, so it does not vote.
    fn branch(&mut self, arms: &[Vec<Event>], span: Span) -> Result<bool, Diagnostic> {
        let before = self.state.clone();
        let mut joined: Option<Vec<State>> = None;
        for arm in arms {
            self.state = before.clone();
            if self.run(arm)? {
                continue;
            }
            self.forget_locals(&before);
            match &joined {
                None => joined = Some(self.state.clone()),
                Some(agreed) => {
                    if *agreed != self.state {
                        let slot = self
                            .differing_slot(agreed)
                            .expect("the states differ, so some slot differs");
                        return Err(Diagnostic::new(
                            format!(
                                "the branches disagree about `{}`: one consumes it and another does not",
                                self.name(slot)
                            ),
                            span,
                        ));
                    }
                }
            }
        }
        match joined {
            Some(agreed) => {
                self.state = agreed;
                Ok(false)
            }
            // Every arm returned, so nothing reaches the merge point.
            None => {
                self.state = before;
                Ok(true)
            }
        }
    }

    /// Drop from the current state every slot that was not yet tracked when
    /// a region was entered.
    ///
    /// Such a slot was declared *inside* the region, so it belongs to a block
    /// that has already closed and already checked it. Without this, an arm
    /// that creates and spends a value of its own would look like a
    /// disagreement with an arm that did nothing, and a loop body that opens
    /// and closes a file each iteration would look like a leak.
    fn forget_locals(&mut self, before: &[State]) {
        for (slot, was) in before.iter().enumerate() {
            if *was == State::Untracked {
                self.state[slot] = State::Untracked;
            }
        }
    }

    fn live_slot(&self) -> Option<Slot> {
        self.state.iter().position(|s| *s == State::Live).map(|i| Slot(i as u32))
    }

    fn differing_slot(&self, other: &[State]) -> Option<Slot> {
        self.state.iter().zip(other).position(|(a, b)| a != b).map(|i| Slot(i as u32))
    }
}
