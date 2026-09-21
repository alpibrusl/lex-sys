//! Byte spans, source files and located diagnostics.
//!
//! Spans live in side tables keyed by node id, never inside AST nodes: the AST
//! is the thing we intend to canonicalise and hash (#1), and a hash must not
//! depend on where in a file the source happened to sit.

/// A half-open byte range `[start, end)` into **the whole program's**
/// source (`docs/many-files.md` §4).
///
/// Not into one file's. With several files an offset alone would not say
/// where an error is, and the obvious fix — a file id in every
/// `Diagnostic` — would touch hundreds of call sites to say something the
/// offset already determines. So each file is given a base offset when it
/// is parsed, and a [`SourceMap`] resolves an offset back to a file, a line
/// and a column at render time. The checker never learns that files exist.
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

/// Every source file of one program, laid end to end
/// (`docs/many-files.md` §4).
///
/// Each file occupies a half-open range of global offsets. Rendering a
/// diagnostic is a binary search for the file that contains its span's
/// start, and then that file's own line lookup, unchanged.
pub struct SourceMap {
    files: Vec<SourceFile>,
    /// The global offset each file begins at, in the same order.
    bases: Vec<u32>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new(), bases: Vec::new() }
    }

    /// Add a file and answer the base offset its spans are relative to.
    ///
    /// Files are separated by one byte that belongs to neither, so the end
    /// of one and the start of the next are never the same offset — an
    /// empty file would otherwise share a base with its neighbour, and a
    /// span at that offset could belong to either.
    pub fn add(&mut self, path: impl Into<String>, text: impl Into<String>) -> u32 {
        let base = self
            .bases
            .last()
            .map_or(0, |&b| b + self.files.last().map_or(0, |f| f.text.len() as u32) + 1);
        self.files.push(SourceFile::new(path, text));
        self.bases.push(base);
        base
    }

    /// The file a global offset falls in, and that offset within it.
    fn locate(&self, offset: u32) -> Option<(&SourceFile, u32)> {
        let index = match self.bases.binary_search(&offset) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let file = self.files.get(index)?;
        Some((file, offset - self.bases[index]))
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
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

    /// `path:line:col: error: message`, followed by the offending line and
    /// a caret, resolved through the whole program's source.
    pub fn render_in(&self, map: &SourceMap) -> String {
        match map.locate(self.span.start) {
            Some((file, local)) => {
                let width = self.span.end.saturating_sub(self.span.start);
                let local = Span::new(local, local + width);
                Diagnostic { message: self.message.clone(), span: local }.render(file)
            }
            // An offset in no file at all: better a message with no
            // location than no message.
            None => format!("error: {}", self.message),
        }
    }

    /// `path:line:col: error: message`, followed by the offending line and a caret.
    ///
    /// The span is an offset into `file` here, not into a whole program —
    /// this is the single-file case and what [`Self::render_in`] reduces
    /// to once it has found the file.
    pub fn render(&self, file: &SourceFile) -> String {
        let (line, col) = file.line_col(self.span.start);
        let text = file.line_text(line);
        let start = col.saturating_sub(1) as usize;
        // A span may cover several lines -- a whole function signature, say
        // -- while the caret sits under only the first of them. So it stops
        // at that line's end rather than running on past it: underlining a
        // multi-line signature with several hundred carets says nothing a
        // reader can use.
        // Counted in characters, because `line_col` counts the column that
        // way and the two have to agree.
        let room = text.chars().count().saturating_sub(start).max(1);
        let width = ((self.span.end.saturating_sub(self.span.start)).max(1) as usize).min(room);
        let pad = " ".repeat(start);
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
    fn a_source_map_locates_an_offset_in_the_right_file() {
        // `docs/many-files.md` §4: a span is an offset into the whole
        // program, and rendering resolves it back to a file.
        let mut map = SourceMap::new();
        let first = map.add("a.ls", "fn a() -> [] int { return 1; }\n");
        let second = map.add("b.ls", "fn b() -> [] int {\n    return oops;\n}\n");
        assert_eq!(first, 0);
        assert!(second > 0, "a second file starts after the first");

        // The `oops` on b.ls line 2, at its own column.
        let local = "fn b() -> [] int {\n    return ".len() as u32;
        let at = Span::new(second + local, second + local + 4);
        let rendered = Diagnostic::new("nope", at).render_in(&map);
        assert!(rendered.starts_with("b.ls:2:12: error: nope"), "{rendered}");
        assert!(rendered.contains("    return oops;"), "{rendered}");
    }

    #[test]
    fn an_empty_file_does_not_share_a_base_with_its_neighbour() {
        // Files are separated by one byte belonging to neither, so a span
        // at a boundary is never ambiguous.
        let mut map = SourceMap::new();
        let a = map.add("a.ls", "");
        let b = map.add("b.ls", "");
        let c = map.add("c.ls", "");
        assert!(a < b && b < c, "{a} {b} {c}");
    }

    #[test]
    fn a_caret_stops_at_the_end_of_its_line() {
        // A multi-line span -- a wrapped function signature is the usual
        // one -- underlines the first line and stops there.
        let file = SourceFile::new("p.ls", "fn wide(\n    a: int,\n) -> [] int { return a; }\n");
        let whole = Span::new(0, file.text.len() as u32);
        let rendered = Diagnostic::new("nope", whole).render(&file);
        let carets = rendered.lines().next_back().expect("a caret line").trim();
        assert_eq!(carets.len(), "fn wide(".len(), "{rendered}");
    }

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
