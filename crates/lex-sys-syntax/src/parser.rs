//! Recursive-descent parser with precedence climbing.
//!
//! The parser refuses on the first error: M0 wants a located refusal, not
//! recovery. Multi-error recovery is M1 work (#1).
//!
//! `Parser`'s `impl` is split by concern across this module's children
//! (`items`, `stmt`, `expr`), which see its private fields the way a
//! child of `lex-sys-ir`'s `lower` sees `FnLowering`'s
//! (`CONTRIBUTING.md`). This file keeps the entry points, the struct,
//! and the token plumbing every child calls.

use crate::ast::*;
use crate::lexer::{Token, TokenKind, tokenize};
use crate::rules::Rule;
use crate::span::{Diagnostic, Span};

pub fn parse(source: &str) -> Result<Ast, Diagnostic> {
    let mut ast = Ast::new();
    parse_into(&mut ast, source, 0)?;
    Ok(ast)
}

/// Parse one file of a program into an AST that may already hold others
/// (`docs/many-files.md` §2).
///
/// `base` is the global offset this file's spans are relative to, as a
/// [`SourceMap`](crate::span::SourceMap) handed it out. The lexer works in
/// local offsets, so every token's span is shifted once here rather than
/// the whole lexer learning about bases — and the one place that slices the
/// source by a span subtracts it again.
pub fn parse_into(ast: &mut Ast, source: &str, base: u32) -> Result<(), Diagnostic> {
    let mut tokens = match tokenize(source) {
        Ok(tokens) => tokens,
        Err(mut error) => {
            error.span = shift(error.span, base);
            return Err(error);
        }
    };
    for token in &mut tokens {
        token.span = shift(token.span, base);
    }
    let owned = std::mem::take(ast);
    let mut p = Parser {
        source,
        base,
        tokens,
        pos: 0,
        ast: owned,
        no_struct_literal: false,
        current_module: 0,
        current_edition: 1,
    };
    let outcome = p.unit();
    *ast = p.ast;
    outcome
}

fn shift(span: Span, base: u32) -> Span {
    Span::new(span.start + base, span.end + base)
}

struct Parser<'a> {
    source: &'a str,
    /// What this file's spans were shifted by, so `text` can undo it.
    base: u32,
    tokens: Vec<Token>,
    pos: usize,
    ast: Ast,
    /// True while parsing the condition of an `if` or `while`, where
    /// `Point { .. }` cannot be told from the body's opening brace. Rust has
    /// the same ambiguity and resolves it the same way: no struct literal
    /// here, and parentheses if you really meant one.
    no_struct_literal: bool,
    /// The module the items being parsed belong to (`docs/modules.md`
    /// §3). Zero is the root, which is where a file that declares nothing
    /// puts things -- so this starts at zero for every file and every
    /// program written before modules is unaffected.
    current_module: u32,
    /// This file's edition (`docs/editions.md` §6.1). One is the
    /// language as it is today, and a file with no `edition N;` marker
    /// stays there forever -- so this starts at one for every file and
    /// every program written before editions existed is unaffected.
    current_edition: u32,
}

mod expr;
mod items;
mod stmt;

#[cfg(test)]
#[path = "parser/tests.rs"]
mod tests;

impl<'a> Parser<'a> {
    // ---- token plumbing ------------------------------------------------

    pub(crate) fn peek(&self) -> Token {
        self.tokens[self.pos]
    }

    /// The kind of the token `n` ahead, saturating at end of file.
    pub(crate) fn peek_kind(&self, n: usize) -> TokenKind {
        self.tokens[(self.pos + n).min(self.tokens.len() - 1)].kind
    }

    pub(crate) fn text(&self, tok: Token) -> &'a str {
        let start = (tok.span.start - self.base) as usize;
        let end = (tok.span.end - self.base) as usize;
        &self.source[start..end]
    }

    pub(crate) fn bump(&mut self) -> Token {
        let tok = self.peek();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        tok
    }

    pub(crate) fn eat(&mut self, kind: TokenKind) -> bool {
        if self.peek().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, kind: TokenKind) -> Result<Token, Diagnostic> {
        let tok = self.peek();
        if tok.kind == kind {
            Ok(self.bump())
        } else {
            Err(self.err(
                Rule::TypeMismatch,
                format!("expected {}, found {}", kind.describe(), tok.kind.describe()),
            ))
        }
    }

    /// A refusal at the token the parser is looking at.
    ///
    /// The rule is passed in rather than derived, for
    /// `docs/agent-errors.md` §3's reason: the site knows which rule it
    /// is enforcing and a regular expression over the sentence does not.
    pub(crate) fn err(&self, rule: Rule, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(rule, message, self.peek().span)
    }
}
