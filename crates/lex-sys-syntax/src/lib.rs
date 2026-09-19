//! Front end of the lex-sys bootstrap compiler: tokens, a canonical-shaped AST,
//! and a recursive-descent parser.
//!
//! The M0 surface is deliberately tiny (#3): integers, functions and calls,
//! arithmetic and comparison, `if`/`else`, `while`, and local bindings. No
//! types beyond `int`, no linearity, no effects — those are M1 and M2.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod span;

pub use ast::Ast;
pub use parser::parse;
pub use span::{Diagnostic, SourceFile, Span};
