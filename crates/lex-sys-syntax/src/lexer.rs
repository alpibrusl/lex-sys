//! Tokeniser.
//!
//! Whitespace and comments are discarded here and never reach the AST: they are
//! formatting, and formatting must not be able to change a content hash.

use crate::span::{Diagnostic, Span};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    // literals and names
    Ident,
    Int,
    // keywords
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    Return,
    True,
    False,
    Struct,
    Enum,
    Match,
    Res,
    Val,
    Extern,
    Borrow,
    Region,
    As,
    In,
    Mut,
    Where,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Semi,
    Colon,
    ColonColon,
    Arrow,
    FatArrow,
    Underscore,
    Eq,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    /// A double-quoted literal. It never becomes a runtime value: the only
    /// places one may appear are an effect label's argument and a narrowing
    /// call, both of which are settled and erased at compile time (§7.4).
    /// Runtime strings are M3, and they are a different thing entirely.
    Str,
    AmpAmp,
    Amp,
    PipePipe,
    Eof,
}

impl TokenKind {
    /// How the token is named in a diagnostic.
    pub fn describe(self) -> &'static str {
        match self {
            TokenKind::Ident => "an identifier",
            TokenKind::Int => "an integer literal",
            TokenKind::Fn => "`fn`",
            TokenKind::Let => "`let`",
            TokenKind::Var => "`var`",
            TokenKind::If => "`if`",
            TokenKind::Else => "`else`",
            TokenKind::While => "`while`",
            TokenKind::Return => "`return`",
            TokenKind::True => "`true`",
            TokenKind::False => "`false`",
            TokenKind::Struct => "`struct`",
            TokenKind::Enum => "`enum`",
            TokenKind::Match => "`match`",
            TokenKind::Res => "`res`",
            TokenKind::Extern => "`extern`",
            TokenKind::Borrow => "`borrow`",
            TokenKind::Region => "`region`",
            TokenKind::As => "`as`",
            TokenKind::In => "`in`",
            TokenKind::Mut => "`mut`",
            TokenKind::Where => "`where`",
            TokenKind::Val => "`val`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::Comma => "`,`",
            TokenKind::Dot => "`.`",
            TokenKind::Semi => "`;`",
            TokenKind::Colon => "`:`",
            TokenKind::ColonColon => "`::`",
            TokenKind::FatArrow => "`=>`",
            TokenKind::Underscore => "`_`",
            TokenKind::Arrow => "`->`",
            TokenKind::Eq => "`=`",
            TokenKind::EqEq => "`==`",
            TokenKind::BangEq => "`!=`",
            TokenKind::Lt => "`<`",
            TokenKind::LtEq => "`<=`",
            TokenKind::Gt => "`>`",
            TokenKind::GtEq => "`>=`",
            TokenKind::Plus => "`+`",
            TokenKind::Minus => "`-`",
            TokenKind::Star => "`*`",
            TokenKind::Slash => "`/`",
            TokenKind::Percent => "`%`",
            TokenKind::Bang => "`!`",
            TokenKind::AmpAmp => "`&&`",
            TokenKind::Amp => "`&`",
            TokenKind::Str => "a string literal",
            TokenKind::PipePipe => "`||`",
            TokenKind::Eof => "end of file",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Tokenise a whole source file. The final token is always `Eof`.
pub fn tokenize(text: &str) -> Result<Vec<Token>, Diagnostic> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];

        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment.
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        let start = i;

        // A string literal. Five escapes and no more (`docs/strings.md` §4):
        // `\u` would be an encoding claim, which §1 declines to make, and
        // `\x` is the bitwise escape hatch §2 is deferring. A backslash
        // before anything else is refused where it is written rather than
        // passed through as itself.
        if b == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\n' {
                    return Err(Diagnostic::new(
                        "a string literal may not span lines",
                        Span::new(start as u32, i as u32),
                    ));
                }
                if bytes[i] == b'\\' {
                    let Some(escape) = bytes.get(i + 1) else { break };
                    if !matches!(escape, b'n' | b't' | b'\\' | b'"' | b'0') {
                        let end = next_char_boundary(text, i + 1);
                        return Err(Diagnostic::new(
                            format!(
                                "`\\{}` is not an escape; a string literal takes `\\n`, `\\t`, `\\\\`, `\\\"` and `\\0`",
                                &text[i + 1..end]
                            ),
                            Span::new(i as u32, end as u32),
                        ));
                    }
                    i += 2;
                    continue;
                }
                i += 1;
            }
            if i == bytes.len() {
                return Err(Diagnostic::new(
                    "unterminated string literal",
                    Span::new(start as u32, bytes.len() as u32),
                ));
            }
            i += 1;
            out.push(Token { kind: TokenKind::Str, span: Span::new(start as u32, i as u32) });
            continue;
        }

        if b.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            // A literal may not run straight into a name: `1x` is a typo, not `1 x`.
            if i < bytes.len() && is_ident_continue(bytes[i]) {
                return Err(Diagnostic::new(
                    "unexpected character in an integer literal",
                    Span::new(i as u32, i as u32 + 1),
                ));
            }
            out.push(Token { kind: TokenKind::Int, span: Span::new(start as u32, i as u32) });
            continue;
        }

        if is_ident_start(b) {
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let kind = keyword(&text[start..i]).unwrap_or(TokenKind::Ident);
            out.push(Token { kind, span: Span::new(start as u32, i as u32) });
            continue;
        }

        let two = |k: TokenKind| (k, 2usize);
        let one = |k: TokenKind| (k, 1usize);
        let next = bytes.get(i + 1).copied();
        let (kind, len) = match (b, next) {
            (b'-', Some(b'>')) => two(TokenKind::Arrow),
            (b'&', Some(b'&')) => two(TokenKind::AmpAmp),
            (b'|', Some(b'|')) => two(TokenKind::PipePipe),
            (b'=', Some(b'=')) => two(TokenKind::EqEq),
            (b'=', Some(b'>')) => two(TokenKind::FatArrow),
            (b':', Some(b':')) => two(TokenKind::ColonColon),
            (b'!', Some(b'=')) => two(TokenKind::BangEq),
            (b'<', Some(b'=')) => two(TokenKind::LtEq),
            (b'>', Some(b'=')) => two(TokenKind::GtEq),
            (b'(', _) => one(TokenKind::LParen),
            (b')', _) => one(TokenKind::RParen),
            (b'{', _) => one(TokenKind::LBrace),
            (b'}', _) => one(TokenKind::RBrace),
            (b'[', _) => one(TokenKind::LBracket),
            (b']', _) => one(TokenKind::RBracket),
            (b',', _) => one(TokenKind::Comma),
            (b'.', _) => one(TokenKind::Dot),
            (b';', _) => one(TokenKind::Semi),
            (b':', _) => one(TokenKind::Colon),
            (b'=', _) => one(TokenKind::Eq),
            (b'<', _) => one(TokenKind::Lt),
            (b'>', _) => one(TokenKind::Gt),
            (b'+', _) => one(TokenKind::Plus),
            (b'-', _) => one(TokenKind::Minus),
            (b'*', _) => one(TokenKind::Star),
            (b'/', _) => one(TokenKind::Slash),
            (b'%', _) => one(TokenKind::Percent),
            (b'!', _) => one(TokenKind::Bang),
            // A lone `&` is a reference (§5); `&&` was already taken above,
            // so `&!r` lexes as three tokens and needs no special case.
            (b'&', _) => one(TokenKind::Amp),
            _ => {
                let end = next_char_boundary(text, i);
                return Err(Diagnostic::new(
                    format!("unexpected character `{}`", &text[i..end]),
                    Span::new(i as u32, end as u32),
                ));
            }
        };
        i += len;
        out.push(Token { kind, span: Span::new(start as u32, i as u32) });
    }

    out.push(Token { kind: TokenKind::Eof, span: Span::new(text.len() as u32, text.len() as u32) });
    Ok(out)
}

fn next_char_boundary(text: &str, i: usize) -> usize {
    let mut end = i + 1;
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    end
}

fn keyword(s: &str) -> Option<TokenKind> {
    Some(match s {
        "fn" => TokenKind::Fn,
        "let" => TokenKind::Let,
        "var" => TokenKind::Var,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "return" => TokenKind::Return,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "match" => TokenKind::Match,
        "res" => TokenKind::Res,
        "extern" => TokenKind::Extern,
        "val" => TokenKind::Val,
        "borrow" => TokenKind::Borrow,
        "region" => TokenKind::Region,
        "as" => TokenKind::As,
        "in" => TokenKind::In,
        "mut" => TokenKind::Mut,
        "where" => TokenKind::Where,
        "_" => TokenKind::Underscore,
        _ => return None,
    })
}

fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}

fn is_ident_continue(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        tokenize(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn keywords_beat_identifiers() {
        assert_eq!(kinds("fn"), vec![TokenKind::Fn, TokenKind::Eof]);
        assert_eq!(kinds("fnord"), vec![TokenKind::Ident, TokenKind::Eof]);
    }

    #[test]
    fn two_character_operators_win() {
        assert_eq!(
            kinds("-> == != <= >= < > ="),
            vec![
                TokenKind::Arrow,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::Lt,
                TokenKind::Gt,
                TokenKind::Eq,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn comments_and_whitespace_vanish() {
        assert_eq!(kinds("1 // two\n2"), vec![TokenKind::Int, TokenKind::Int, TokenKind::Eof]);
    }

    #[test]
    fn spans_point_back_into_the_source() {
        let src = "let x";
        let toks = tokenize(src).unwrap();
        assert_eq!(&src[toks[1].span.start as usize..toks[1].span.end as usize], "x");
    }

    #[test]
    fn a_string_literal_is_one_token() {
        assert_eq!(kinds("\"libc\""), vec![TokenKind::Str, TokenKind::Eof]);
        let src = "narrow(f, \"libc\")";
        let toks = tokenize(src).unwrap();
        let literal = toks.iter().find(|t| t.kind == TokenKind::Str).expect("the literal");
        assert_eq!(&src[literal.span.start as usize..literal.span.end as usize], "\"libc\"");
    }

    #[test]
    fn an_unterminated_string_is_refused_rather_than_running_to_the_end() {
        assert!(tokenize("\"libc").is_err());
        assert!(tokenize("\"lib\nc\"").is_err());
    }

    #[test]
    fn a_literal_may_not_run_into_a_name() {
        assert!(tokenize("1x").is_err());
    }

    #[test]
    fn unknown_characters_are_refused() {
        let err = tokenize("a ^ b").unwrap_err();
        assert!(err.message.contains('^'), "{}", err.message);
    }

    #[test]
    fn a_non_ascii_character_is_refused_without_splitting_it() {
        let err = tokenize("π").unwrap_err();
        assert!(err.message.contains('π'), "{}", err.message);
    }
}
