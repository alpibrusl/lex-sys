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
/// A type parameter is `val`: there is no mode polymorphism in M2 (§12), and
/// a generic function instantiated at a `res` type is refused at the call
/// site instead. A generic *type* is another matter — `Pair[File]` is `res`
/// and `Pair[int]` is `val`, because the arguments are substituted in first.
pub(crate) fn mode_of(defs: &[TypeDef], unifier: &Unifier, ty: &Type) -> Mode {
    match unifier.resolve(ty) {
        Type::Named(def, args) => {
            let index = def.0 as usize;
            if let Some(declared) = defs[index].declared_mode {
                return declared;
            }
            // The type graph is acyclic — `collect_types` refused anything
            // else — so this recursion terminates without a seen set.
            let members: Vec<Type> = defs[index].members().cloned().collect();
            for member in members {
                if mode_of(defs, unifier, &member.substitute(&args, &[])) == Mode::Res {
                    return Mode::Res;
                }
            }
            Mode::Val
        }
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
    Declare { slot: Slot, name: String, span: Span },
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
    /// A `borrow` block opened over this slot (§5 rule 1). Frozen means not
    /// movable and not consumable — a read is still fine, which is the whole
    /// point, and for a `val` slot a read was never a move so nothing
    /// changes.
    Freeze { slot: Slot, span: Span },
    /// The block closed and the slot is owned again. §5: "a three-valued flag
    /// set at block entry and restored at block exit".
    Thaw { slot: Slot },
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
    slots: &'a [Type],
    /// Filled in by `Declare`, so an error can name the binding it is about.
    names: Vec<Option<(String, Span)>>,
    state: Vec<State>,
    /// How many `borrow` blocks are open over each slot. A count rather than
    /// a flag because shared borrows nest (§5's `two_shared_borrows`), and
    /// the innermost one closing must not thaw the whole thing.
    frozen: Vec<u32>,
}

/// Check one function body's trace. `slots` are the settled slot types.
pub(crate) fn check(
    defs: &[TypeDef],
    unifier: &Unifier,
    slots: &[Type],
    events: &[Event],
) -> Result<(), Diagnostic> {
    let mut check = Check {
        defs,
        unifier,
        slots,
        names: vec![None; slots.len()],
        state: vec![State::Untracked; slots.len()],
        frozen: vec![0; slots.len()],
    };
    check.run(events)?;
    Ok(())
}

impl Check<'_> {
    fn is_res(&self, slot: Slot) -> bool {
        mode_of(self.defs, self.unifier, &self.slots[slot.0 as usize]) == Mode::Res
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
                Event::Declare { slot, name, span } => {
                    self.names[slot.0 as usize] = Some((name.clone(), *span));
                    self.state[slot.0 as usize] =
                        if self.is_res(*slot) { State::Live } else { State::Untracked };
                }
                Event::Freeze { slot, span } => {
                    if self.state[slot.0 as usize] == State::Moved {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` has already been consumed; there is nothing left to borrow",
                                self.name(*slot)
                            ),
                            *span,
                        ));
                    }
                    self.frozen[slot.0 as usize] += 1;
                }
                Event::Thaw { slot } => {
                    self.frozen[slot.0 as usize] -= 1;
                }
                Event::Use { slot, span } => match self.state[slot.0 as usize] {
                    State::Untracked => {}
                    // Reading a `res` binding *is* moving it — this slice has
                    // no non-owning read of an owned name — so a frozen one
                    // cannot be read at all. §5 rule 1: frozen means not
                    // movable, not consumable.
                    State::Live if self.frozen[slot.0 as usize] > 0 => {
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
                    if self.frozen[slot.0 as usize] > 0 {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is frozen by an enclosing `borrow`, so it cannot be assigned to here",
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
                    if mode_of(self.defs, self.unifier, ty) == Mode::Res {
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
                    if mode_of(self.defs, self.unifier, ty) == Mode::Res {
                        return Err(Diagnostic::new(
                            format!(
                                "`{}` is `res`, so a field cannot be read out of it; take the whole value apart with a destructuring `let` (a non-owning read is a borrow, which is §5)",
                                self.unifier.display(&self.unifier.resolve(ty))
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
