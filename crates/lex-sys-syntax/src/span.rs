//! Byte spans, source files and located diagnostics.
//!
//! Spans live in side tables keyed by node id, never inside AST nodes: the AST
//! is the thing we intend to canonicalise and hash (#1), and a hash must not
//! depend on where in a file the source happened to sit.

/// A half-open byte range `[start, end)` into a single source file.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// The span covering `self` and `other` and everything between them.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// A source file plus a precomputed line index.
pub struct SourceFile {
    pub path: String,
    pub text: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self { path: path.into(), text, line_starts }
    }

    /// 1-based line and column (columns counted in characters, not bytes).
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line] as usize;
        let upto = &self.text[start..(offset as usize).min(self.text.len())];
        (line as u32 + 1, upto.chars().count() as u32 + 1)
    }

    /// The text of a 1-based line, without its trailing newline.
    pub fn line_text(&self, line: u32) -> &str {
        let idx = (line as usize).saturating_sub(1);
        let start = self.line_starts.get(idx).copied().unwrap_or(0) as usize;
        let end = self.line_starts.get(idx + 1).map(|&e| e as usize - 1).unwrap_or(self.text.len());
        self.text[start..end.min(self.text.len())].trim_end_matches('\r')
    }
}

/// A located compiler error. lex-sys has no warnings: a diagnostic is a refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self { message: message.into(), span }
    }

    /// `path:line:col: error: message`, followed by the offending line and a caret.
    pub fn render(&self, file: &SourceFile) -> String {
        let (line, col) = file.line_col(self.span.start);
        let text = file.line_text(line);
        let width = (self.span.end.saturating_sub(self.span.start)).max(1) as usize;
        let pad = " ".repeat(col.saturating_sub(1) as usize);
        format!(
            "{}:{}:{}: error: {}\n    {}\n    {}{}",
            file.path,
            line,
            col,
            self.message,
            text,
            pad,
            "^".repeat(width)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based() {
        let f = SourceFile::new("t.ls", "ab\ncde\n");
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(2), (1, 3));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(5), (2, 3));
    }

    #[test]
    fn line_text_drops_the_newline() {
        let f = SourceFile::new("t.ls", "ab\ncde\n");
        assert_eq!(f.line_text(1), "ab");
        assert_eq!(f.line_text(2), "cde");
    }

    #[test]
    fn render_points_at_the_span() {
        let f = SourceFile::new("t.ls", "fn main() -> [] int {\n    let x = ;\n}\n");
        let d = Diagnostic::new("expected an expression", Span::new(34, 35));
        let out = d.render(&f);
        assert!(out.starts_with("t.ls:2:13: error: expected an expression"), "{out}");
        assert!(out.ends_with("^"), "{out}");
    }
}
