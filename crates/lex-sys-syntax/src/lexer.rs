//! Tokeniser.
//!
//! Whitespace and comments are discarded here and never reach the AST: they are
//! formatting, and formatting must not be able to change a content hash.

use crate::rules::Rule;
use crate::span::{Diagnostic, Span};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    // literals and names
    Ident,
    Int,
    /// `1.0`, `2.5e-3`, `1e9` (`docs/floating-point.md` §1).
    ///
    /// Needs a decimal point with digits on *both* sides, or an
    /// exponent. That is what keeps it from colliding with `t.0`, the
    /// tuple component: an index follows a name and a float follows a
    /// digit, so the character before the dot decides.
    Float,
    // keywords
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    Return,
    /// `defer E;` — run `E` at every exit from this block (`docs/defer.md`).
    Defer,
    True,
    False,
    Struct,
    Enum,
    Match,
    Res,
    Val,
    Extern,
    /// `module a.b;` and `import a.b;` (`docs/modules.md`).
    Module,
    Import,
    /// `pub` — reachable from another module. Never *safe*: §6 says a
    /// module is not a trust boundary, and `pub` does not grant authority.
    Pub,
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
    /// `..` — the half-open range in `s[a..b]` (`docs/slicing.md` §1).
    DotDot,
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
    /// `|`, `^`, `~`, `<<` and `>>` — the bit operators (`docs/bitwise.md`).
    ///
    /// `&` needs no token of its own: `Amp` above already exists for the
    /// reference constructor, and §5.1 is why the two cannot be confused.
    Pipe,
    Caret,
    Tilde,
    LtLt,
    GtGt,
    Eof,
}

impl TokenKind {
    /// How the token is named in a diagnostic.
    pub fn describe(self) -> &'static str {
        match self {
            TokenKind::Ident => "an identifier",
            TokenKind::Int => "an integer literal",
            TokenKind::Float => "a floating-point literal",
            TokenKind::Fn => "`fn`",
            TokenKind::Let => "`let`",
            TokenKind::Var => "`var`",
            TokenKind::If => "`if`",
            TokenKind::Else => "`else`",
            TokenKind::While => "`while`",
            TokenKind::Return => "`return`",
            TokenKind::Defer => "`defer`",
            TokenKind::True => "`true`",
            TokenKind::False => "`false`",
            TokenKind::Struct => "`struct`",
            TokenKind::Enum => "`enum`",
            TokenKind::Match => "`match`",
            TokenKind::Res => "`res`",
            TokenKind::Extern => "`extern`",
            TokenKind::Module => "`module`",
            TokenKind::Import => "`import`",
            TokenKind::Pub => "`pub`",
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
            TokenKind::DotDot => "`..`",
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
            TokenKind::Pipe => "`|`",
            TokenKind::Caret => "`^`",
            TokenKind::Tilde => "`~`",
            TokenKind::LtLt => "`<<`",
            TokenKind::GtGt => "`>>`",
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

        // A string literal. Six escapes and no more (`docs/strings.md` §4):
        // `\u` would be an encoding claim, which §1 declines to make, and
        // `\x` is the bitwise escape hatch §2 is deferring. A backslash
        // before anything else is refused where it is written rather than
        // passed through as itself.
        //
        // `\r` is the sixth, and it was added because a program needed it:
        // `examples/serve/` speaks HTTP, whose line ending is CRLF, and
        // without it a protocol's own separator had to be written as
        // `byte_of(13)` into a buffer (`docs/reach.md` §4).
        if b == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\n' {
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        "a string literal may not span lines",
                        Span::new(start as u32, i as u32),
                    ));
                }
                if bytes[i] == b'\\' {
                    let Some(escape) = bytes.get(i + 1) else { break };
                    if !matches!(escape, b'n' | b'r' | b't' | b'\\' | b'"' | b'0') {
                        let end = next_char_boundary(text, i + 1);
                        return Err(Diagnostic::new(
                            Rule::UnknownEscape,
                            format!(
                                "`\\{}` is not an escape; a string literal takes `\\n`, `\\r`, `\\t`, `\\\\`, `\\\"` and `\\0`",
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
                    Rule::LiteralForm,
                    "unterminated string literal",
                    Span::new(start as u32, bytes.len() as u32),
                ));
            }
            i += 1;
            out.push(Token { kind: TokenKind::Str, span: Span::new(start as u32, i as u32) });
            continue;
        }

        // A character literal: the third spelling of an integer, after
        // decimal and hexadecimal (`docs/character-literals.md` §2). It
        // lexes as `TokenKind::Int` because that is what it is — `'a'` and
        // `97` are one node — so nothing past `int_value` knows the
        // spelling exists, and no hash moved when it arrived (§5).
        //
        // The escape set is the string's six with `\'` where `\"` is: each
        // literal escapes its own delimiter and not the other's.
        if b == b'\'' {
            let mut j = i + 1;
            match bytes.get(j) {
                None => {
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        "unterminated character literal",
                        Span::new(start as u32, bytes.len() as u32),
                    ));
                }
                Some(b'\'') => {
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        "a character literal holds exactly one character; `''` holds none",
                        Span::new(start as u32, (j + 1) as u32),
                    ));
                }
                Some(b'\n') => {
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        "a character literal may not span lines",
                        Span::new(start as u32, j as u32),
                    ));
                }
                Some(b'\\') => {
                    let Some(escape) = bytes.get(j + 1) else {
                        return Err(Diagnostic::new(
                            Rule::LiteralForm,
                            "unterminated character literal",
                            Span::new(start as u32, bytes.len() as u32),
                        ));
                    };
                    if !matches!(escape, b'n' | b'r' | b't' | b'\\' | b'\'' | b'0') {
                        let end = next_char_boundary(text, j + 1);
                        return Err(Diagnostic::new(
                            Rule::UnknownEscape,
                            format!(
                                "`\\{}` is not an escape; a character literal takes `\\n`, `\\r`, `\\t`, `\\\\`, `\\'` and `\\0`",
                                &text[j + 1..end]
                            ),
                            Span::new(j as u32, end as u32),
                        ));
                    }
                    j += 2;
                }
                Some(c) if c.is_ascii() => j += 1,
                Some(_) => {
                    // `'é'` is two bytes in UTF-8 and this literal is one.
                    // Refused rather than decoded, because U+00E9 and its
                    // first encoded byte are both obvious readings and
                    // `strings.md` §1 declines to pick (§3.1).
                    let end = next_char_boundary(text, j);
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        format!(
                            "`{}` is not ASCII, and a character literal is one byte; write the code point, or the bytes of its encoding",
                            &text[j..end]
                        ),
                        Span::new(start as u32, end as u32),
                    ));
                }
            }
            if bytes.get(j) == Some(&b'\'') {
                j += 1;
            } else {
                // Which mistake it is depends on whether the quote ever
                // closes: `'ab'` is two characters and `'a` is a literal
                // that never ended. The string lexer draws the same line
                // between "spans lines" and "unterminated".
                let mut scan = j;
                while scan < bytes.len() && bytes[scan] != b'\n' && bytes[scan] != b'\'' {
                    scan += 1;
                }
                let closed = bytes.get(scan) == Some(&b'\'');
                return Err(Diagnostic::new(
                    Rule::LiteralForm,
                    if closed {
                        "a character literal holds exactly one character; a longer run of text is a string, written with `\"`"
                    } else {
                        "unterminated character literal"
                    },
                    Span::new(start as u32, if closed { scan + 1 } else { scan } as u32),
                ));
            }
            i = j;
            out.push(Token { kind: TokenKind::Int, span: Span::new(start as u32, i as u32) });
            continue;
        }

        if b.is_ascii_digit() {
            // `0x` is the one prefix. Base two and base eight were left out
            // deliberately: a mask is written in hex by everyone who writes
            // masks, and `0o`'s only customer is a Unix file mode, which
            // this language cannot yet set (`docs/bitwise.md` §1.1).
            let hex = b == b'0' && matches!(bytes.get(i + 1), Some(b'x'));
            if hex {
                i += 2;
                if !bytes.get(i).is_some_and(|c| c.is_ascii_hexdigit()) {
                    return Err(Diagnostic::new(
                        Rule::LiteralForm,
                        "`0x` needs at least one hexadecimal digit",
                        Span::new(start as u32, i as u32),
                    ));
                }
            }
            while i < bytes.len()
                && ((if hex { bytes[i].is_ascii_hexdigit() } else { bytes[i].is_ascii_digit() })
                    || bytes[i] == b'_')
            {
                i += 1;
            }

            // A floating-point literal, if what follows says so
            // (`docs/floating-point.md` §1). Never after `0x`, where `e`
            // is a digit and `.` is nothing -- and never when this number
            // is itself a tuple index, because `t.0.1` is two components
            // and not one component and a float.
            //
            // That last case is the one bit of context the rule needs: a
            // number *immediately after a dot* is an index, so it does
            // not get to start a float of its own.
            let after_dot = out.last().is_some_and(|t: &Token| t.kind == TokenKind::Dot);
            let mut float = false;
            if !hex && !after_dot {
                // `.` only counts with a digit after it, which is what
                // leaves `t.0` and a bare `1.` alone.
                if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                    float = true;
                    i += 1;
                    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        i += 1;
                    }
                }
                if matches!(bytes.get(i), Some(b'e' | b'E')) {
                    let mut after = i + 1;
                    if matches!(bytes.get(after), Some(b'+' | b'-')) {
                        after += 1;
                    }
                    if bytes.get(after).is_some_and(u8::is_ascii_digit) {
                        float = true;
                        i = after;
                        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                            i += 1;
                        }
                    }
                }
            }
            // A literal may not run straight into a name: `1x` is a typo, not `1 x`.
            if i < bytes.len() && is_ident_continue(bytes[i]) {
                return Err(Diagnostic::new(
                    Rule::UnexpectedCharacter,
                    "unexpected character in an integer literal",
                    Span::new(i as u32, i as u32 + 1),
                ));
            }
            let kind = if float { TokenKind::Float } else { TokenKind::Int };
            out.push(Token { kind, span: Span::new(start as u32, i as u32) });
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
            (b'.', Some(b'.')) => two(TokenKind::DotDot),
            (b'!', Some(b'=')) => two(TokenKind::BangEq),
            (b'<', Some(b'=')) => two(TokenKind::LtEq),
            (b'>', Some(b'=')) => two(TokenKind::GtEq),
            // Generic arguments are written in brackets, so `>>` is never
            // the end of two type argument lists (`docs/bitwise.md` §5.1).
            (b'<', Some(b'<')) => two(TokenKind::LtLt),
            (b'>', Some(b'>')) => two(TokenKind::GtGt),
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
            (b'|', _) => one(TokenKind::Pipe),
            (b'^', _) => one(TokenKind::Caret),
            (b'~', _) => one(TokenKind::Tilde),
            // A lone `&` is a reference (§5); `&&` was already taken above,
            // so `&!r` lexes as three tokens and needs no special case.
            (b'&', _) => one(TokenKind::Amp),
            _ => {
                let end = next_char_boundary(text, i);
                return Err(Diagnostic::new(
                    Rule::UnexpectedCharacter,
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
        "defer" => TokenKind::Defer,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "match" => TokenKind::Match,
        "res" => TokenKind::Res,
        "extern" => TokenKind::Extern,
        "module" => TokenKind::Module,
        "import" => TokenKind::Import,
        "pub" => TokenKind::Pub,
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
        // `@` rather than `^`: this test used `^` until `docs/bitwise.md`
        // gave `^` a meaning, at which point it started testing nothing.
        // A test whose subject is "this is not a token" has a shelf life,
        // and `@` has no pending design that wants it
        // (`docs/porting.md` §5).
        let err = tokenize("a @ b").unwrap_err();
        assert!(err.message.contains('@'), "{}", err.message);
    }

    #[test]
    fn a_non_ascii_character_is_refused_without_splitting_it() {
        let err = tokenize("π").unwrap_err();
        assert!(err.message.contains('π'), "{}", err.message);
    }
}
