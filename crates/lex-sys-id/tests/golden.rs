//! Golden hashes — what the encoder actually emits, written down.
//!
//! # Why this file exists
//!
//! The 61 tests beside the encoder are all *relational*: renaming a local
//! does not change the body, formatting does not reach the hash, an index is
//! not a range. Every one of them compares two hashes to each other. Not one
//! of them says what a hash **is**.
//!
//! That left `docs/canonical-ast.md` §8's central claim unobservable. §8 says
//! tag values are *"stable within a build and not yet frozen across
//! releases"* and that *"no claim is made that a hash from this build matches
//! one from any other build"* — which is honest, and which nothing could have
//! detected either way. A hash that nothing pins can move without anyone
//! noticing, including the person who moved it.
//!
//! So this file pins them. It is **not** a freeze, and a failure here is not
//! a bug report.
//!
//! # What to do when this test fails
//!
//! Read the diff and decide which of two things happened.
//!
//! * **You changed the encoding on purpose** — added a node kind, renumbered
//!   a tag, canonicalised something that used to be written verbatim. Then
//!   the new hashes are correct: update the table below, and say in the
//!   commit message that identities moved and why. That sentence is the
//!   whole point of this file. Anyone reasoning about whether §8 can ever be
//!   emptied needs the *rate* these move at, and the commit log is where
//!   that record accumulates.
//!
//! * **You did not** — you changed the parser, an unrelated pass, a
//!   dependency. Then something reached the hash that should not have, and
//!   this caught it. That is the bug.
//!
//! The one thing not to do is update the table without deciding which.
//!
//! The failure prints the new table one row per line; `cargo fmt` re-wraps it
//! afterwards, so paste and then format.
//!
//! # Why these fixtures
//!
//! One per node family the encoder has a tag for, kept as small as a
//! fixture can be while still reaching the tag. Small matters: when a hash
//! moves, the set of fixtures that moved with it says roughly where.

use lex_sys_id::identify;
use lex_sys_syntax::parse;

/// `(fixture name, source, declaration to read)`.
///
/// Source is a single line per fixture on purpose — formatting does not reach
/// the hash (there is a test for that next door), so the shape here is chosen
/// for reading rather than to stand for anything.
const FIXTURES: &[(&str, &str, &str)] = &[
    ("empty-row", "fn f() -> [] int { return 0; }", "f"),
    ("int-literal", "fn f() -> [] int { return 7; }", "f"),
    ("bool-literal", "fn f() -> [] bool { return true; }", "f"),
    ("float-literal", "fn f() -> [] float { return 1.5; }", "f"),
    ("arithmetic", "fn f(a: int, b: int) -> [] int { return a * b - 1; }", "f"),
    ("bitwise", "fn f(a: int) -> [] int { return (a << 2) ^ 0x3f; }", "f"),
    // `!` and `~` rather than `-`: there is no unary minus to reach for
    // here. `0 - a` is a *binary* subtract, which is how this fixture was
    // first written and what the golden table caught -- perturbing the
    // BINARY tag moved it, so it had never covered UNARY at all.
    ("unary-not", "fn f(a: bool) -> [] bool { return !a; }", "f"),
    ("unary-bit-not", "fn f(a: int) -> [] int { return ~a; }", "f"),
    ("unary-deref", "fn f[&r](s: &r int) -> [] int { return *s; }", "f"),
    ("comparison", "fn f(a: int, b: int) -> [] bool { return a <= b; }", "f"),
    ("call", "fn g(n: int) -> [] int { return n; } fn f() -> [] int { return g(1); }", "f"),
    ("recursion", "fn f(n: int) -> [] int { if n < 2 { return 1; } return n * f(n - 1); }", "f"),
    ("let-and-local", "fn f() -> [] int { let x = 1; return x; }", "f"),
    ("var-and-assign", "fn f() -> [] int { var x = 1; x = 2; return x; }", "f"),
    ("while-loop", "fn f() -> [] int { var i = 0; while i < 3 { i = i + 1; } return i; }", "f"),
    ("if-else", "fn f(a: bool) -> [] int { if a { return 1; } else { return 2; } }", "f"),
    ("struct-decl", "struct P { x: int, y: int }", "P"),
    ("struct-decl-res", "res struct F { fd: int }", "F"),
    (
        "struct-literal",
        "struct P { x: int, y: int } fn f() -> [] P { return P { x: 1, y: 2 }; }",
        "f",
    ),
    ("field-read", "struct P { x: int, y: int } fn f(p: P) -> [] int { return p.x; }", "f"),
    (
        "destructure",
        "struct P { x: int, y: int } fn f(p: P) -> [] int { let P { x, y } = p; return x; }",
        "f",
    ),
    ("enum-decl", "enum Shape { Circle(int), Empty }", "Shape"),
    (
        "match-arms",
        "enum S { A(int), B } fn f(s: S) -> [] int { match s { S::A(n) => { return n; } S::B => { return 0; } } }",
        "f",
    ),
    ("tuple", "fn f() -> [] (int, int) { return (1, 2); }", "f"),
    ("tuple-field", "fn f(t: (int, int)) -> [] int { return t.0; }", "f"),
    ("reference", "fn f[&r](s: &r int) -> [] int { return 0; }", "f"),
    ("unique-reference", "fn f[&r](s: &!r int) -> [] int { return 0; }", "f"),
    ("slice-index", "fn f[&r](s: &r [int]) -> [] int { return s[0]; }", "f"),
    ("slice-range", "fn f[&r](s: &r [int]) -> [] int { return len(s[1..2]); }", "f"),
    ("generic", "fn f[T](x: T) -> [] T { return x; }", "f"),
    ("effect-row", "fn f[&i](io: &!i Io) -> [io_write] int { return putchar(io, 65); }", "f"),
    (
        "effect-row-argument",
        "fn f[&f2](x: &f2 Ffi(\"libc\")) -> [ffi(\"libc\")] int { return 0; }",
        "f",
    ),
    ("extern-decl", "extern fn e[&f](x: &f Ffi(\"libc\"), n: int) -> [ffi(\"libc\")] int;", "e"),
    (
        "region",
        "fn f() -> [] int { region a { let s = alloc_slice[a](2, 0); return len(s); } }",
        "f",
    ),
    ("static-item", "static t: [int] { let s = alloc_slice[static](2, 0); return s; }", "t"),
];

/// The hashes as of the commit that last touched them.
///
/// `(fixture name, sig, body)`. A type declaration has one identity, so its
/// two columns are the same hash; so does an `extern`, for the reason the
/// encoder gives — there is no body a caller could fail to notice.
const GOLDEN: &[(&str, &str, &str)] = &[
    (
        "empty-row",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "1a4cf7f417cb9d97bd59802cb3ea01d0e90527e7df022f19f662d325da3ebe78",
    ),
    (
        "int-literal",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "a51a5a2b97e9b7004bed56e464323f0cdb8a504d28ceeba9dbbbe7433fe18bb0",
    ),
    (
        "bool-literal",
        "1a92455a8184f451feadbc4e4866124ede02283cb501f81efe5a9c62dfd096f5",
        "192837fd838a77728907c2f57c3e21c2fc2e8a369d506bf6fb6a301d0c2def37",
    ),
    (
        "float-literal",
        "34b9e2570e6fa285edf96a9258575a377ea3819e2d8c7d6e829b8c8533cfbd01",
        "5f7f5a93b8d7a4aac06c193411ddbc1f9e02bfc31622633fa5135c5f06963481",
    ),
    (
        "arithmetic",
        "bd30e7a7635f80351dd2f8775f9ca5d157e78273ea78a42221c288347753b953",
        "5ccbc1e9be962330748af5bfa7d2a6e33b147a4b982712edd49ece154b5c65ec",
    ),
    (
        "bitwise",
        "29c31dd0894cdbda132abcec1c4ec799ea216ceab30171de0495bd9edf461e55",
        "4eff231259036c1e3136e38729a9f39450801a19df51496ea4d26b4dafa46912",
    ),
    (
        "unary-not",
        "8eab39cfeaa1ecf2b6eb14e9e58f34faa38810632d0123dad3370ccff17d3d67",
        "a35fc54e12693dcf96044ba98a7a8877efd8806b893ab3b4a924bc7a38e5f770",
    ),
    (
        "unary-bit-not",
        "29c31dd0894cdbda132abcec1c4ec799ea216ceab30171de0495bd9edf461e55",
        "e148b576f2d2e4c450f87c3a33b11f6810adf5fc7d36fd2cbf98352347db603e",
    ),
    (
        "unary-deref",
        "26c5f988e603fbaf882add4df4f2c4e8ec3cd727a41e9c4a4cea7c198611df19",
        "7ec826f9fd3f122c114f80ad2be0deb46296b0504eb14e20a65ca94498c6286f",
    ),
    (
        "comparison",
        "10de3f90a47607bb046a1ce962f174894cf1d024c316568e7ba126f87d641f6c",
        "808bcf79831575ce737a65ce3129511e1f8ff8f62288b997b63d7f7c065bdd5e",
    ),
    (
        "call",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "932161d6d423e730a8811500ee4f346b46028390b85a3dbd3001d0927ec12d4d",
    ),
    (
        "recursion",
        "29c31dd0894cdbda132abcec1c4ec799ea216ceab30171de0495bd9edf461e55",
        "a4769b71068bcea3d9a959ce138075b73f0ee6b2d5933b35a6c8e4349f40a820",
    ),
    (
        "let-and-local",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "502a5471bb1a5685e5ce7dffd20cff1e213c3c7c2657854c3a64a6ba1de0280c",
    ),
    (
        "var-and-assign",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "58585c9ade55388c62ff4762b44c121117591cf6a09564fa1c506ac5f6840055",
    ),
    (
        "while-loop",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "eac031d90494747061f252bc58cccaa6029b3b1cb459885f925917fa8dbce28d",
    ),
    (
        "if-else",
        "4ac116390d49a1f29d477d12b27eeb658cb23557868560383e32d339a2f6b821",
        "23261f90a13912befd3053609470df703f3eee5055f9695d809a53753fb6dd70",
    ),
    (
        "struct-decl",
        "7e5c3e7d7706436cd7ce67f70057947aa8daca794d6cbc1b743ae4cdef24015c",
        "7e5c3e7d7706436cd7ce67f70057947aa8daca794d6cbc1b743ae4cdef24015c",
    ),
    (
        "struct-decl-res",
        "431783df0c2d2be50de5a19212f67906182e7a9acb8f3b3b3525321b5f95d4d0",
        "431783df0c2d2be50de5a19212f67906182e7a9acb8f3b3b3525321b5f95d4d0",
    ),
    (
        "struct-literal",
        "d0e53528fb662e8e105e5fbb2bc61be949dcef57e9d574679b8c2049c2dcfeec",
        "ec1d7c9536cb63ffacca33cf2bd20abdf6417a3dac7a021b2310c8abc153b6d9",
    ),
    (
        "field-read",
        "c1556b50a0eef5acaf6bd1e0456429cb0e4d1a940865ac55bc1ea8a319e5997b",
        "a0966d18960ef859fdec8d7de6307d391f77ba794ca40f6f0ea3306cf113e074",
    ),
    (
        "destructure",
        "c1556b50a0eef5acaf6bd1e0456429cb0e4d1a940865ac55bc1ea8a319e5997b",
        "9978c985bff416f01772a6224d98510ad3f55908b89860af7488e5059f59744e",
    ),
    (
        "enum-decl",
        "2b0e4fcd51cd581a7f1f3389c6c8a365233d68b04683e06368775c619cdbb226",
        "2b0e4fcd51cd581a7f1f3389c6c8a365233d68b04683e06368775c619cdbb226",
    ),
    (
        "match-arms",
        "99b24ce961c6371ca45ff26d6848c7e5cbbfa7289cb5d7f1ac27a2f52e34de36",
        "13e6abb2ae6ed271070ae32edd2196454b127f00f5087689efa97eef913b187a",
    ),
    (
        "tuple",
        "e65b0ea9df8cef821732dd6be4e61e87b828eace84d86a65ecebfae802162ddf",
        "3bff883b72aedb8256591591f5a1ef2e25d26cdd177a61477bea33c4f5f3ab67",
    ),
    (
        "tuple-field",
        "e1301771a7c83c1d4e21807a4ed487339befaefff4e9284fe240a2444db26f27",
        "aa8800d28ebcb31d07971d229587c2a941ed1b539e6e314ffc1d15ee60913868",
    ),
    (
        "reference",
        "26c5f988e603fbaf882add4df4f2c4e8ec3cd727a41e9c4a4cea7c198611df19",
        "1a4cf7f417cb9d97bd59802cb3ea01d0e90527e7df022f19f662d325da3ebe78",
    ),
    (
        "unique-reference",
        "c8b9e9210fba1c9883cc1b852e4d3b7a66772da4d7d31f4e52659fb20d1a0360",
        "1a4cf7f417cb9d97bd59802cb3ea01d0e90527e7df022f19f662d325da3ebe78",
    ),
    (
        "slice-index",
        "d3378e83ec0fd9bf6a2618b0d833d56cfbd768db0aef17dd779f7b2c0b2db2a3",
        "24efd6e79fefad3e62099168bc25608c990f904adf33d275e4a3081fa8c4bb9b",
    ),
    (
        "slice-range",
        "d3378e83ec0fd9bf6a2618b0d833d56cfbd768db0aef17dd779f7b2c0b2db2a3",
        "e07d84a2c43b7db570ef1d4e96619152c8a715081ecd08bd84077f783a793eaa",
    ),
    (
        "generic",
        "0229d3984262e8c925482425374d99fe18dcaab3dbbda083412625891604785e",
        "80e454604610ef17767e5ec03378aa04cb12296b4e3a45da4d423227b6fc319d",
    ),
    (
        "effect-row",
        "2378b4a5e523b98b752d69e70482e0cf0dfb5d8070c878990745685d587522fd",
        "e7b45e919ac2ea13d5c2b078509f650f75de0c4517389a002b324f08798bf05f",
    ),
    (
        "effect-row-argument",
        "aecb91d32c014b617b5793b98a4a7c306032d5d3423598ec6615b96ade19576b",
        "1a4cf7f417cb9d97bd59802cb3ea01d0e90527e7df022f19f662d325da3ebe78",
    ),
    (
        "extern-decl",
        "cb70d85ce151295247b02bc1e014edef9a3ebb13ee9daa9a10dca0522995cd9a",
        "cb70d85ce151295247b02bc1e014edef9a3ebb13ee9daa9a10dca0522995cd9a",
    ),
    (
        "region",
        "e353cda29e8b90bd2564e597f0a9e28908dbd11bde0c512af5b4fe47a9153fc5",
        "9ed049be8ca87a24e9e98136987b5e0ffa02c6e5aa4084dbc2ff6e515c6afeb0",
    ),
    (
        "static-item",
        "c85a3352f004fa6e116444903f278f08144c4e9a277b3473d5648f5d64e7e576",
        "6b8702d8e58cf7bdba0a0bdf4645f8f22908ff9c6abf93a767ba2b1563dcab18",
    ),
];

#[test]
fn the_encoder_emits_what_it_emitted_before() {
    let mut actual: Vec<(String, String, String)> = Vec::new();
    for (name, source, decl) in FIXTURES {
        let ast = parse(source).unwrap_or_else(|e| panic!("`{name}` should parse: {e:?}"));
        let ids = identify(&ast);
        let (sig, body) = match ids.function(decl) {
            Some(f) => (f.sig.to_hex(), f.body.to_hex()),
            None => {
                let t = ids
                    .type_decl(decl)
                    .unwrap_or_else(|| panic!("`{name}` should declare `{decl}`"));
                (t.id.to_hex(), t.id.to_hex())
            }
        };
        actual.push(((*name).to_owned(), sig, body));
    }

    if GOLDEN.is_empty() {
        panic!("GOLDEN is empty; paste this in:\n\n{}", render(&actual));
    }

    // Matched by fixture *name*, never by position. Adding a fixture in the
    // middle would otherwise report every row below it as changed, which is
    // the one thing this test must not do: the list of what moved is the
    // signal, and a list that cries wolf on an insertion would be ignored
    // within two slices.
    let mut moved: Vec<String> = Vec::new();
    let mut added: Vec<&str> = Vec::new();
    for (name, sig, body) in &actual {
        match GOLDEN.iter().find(|(n, _, _)| n == name) {
            Some((_, s, b)) if s == sig && b == body => {}
            Some(_) => moved.push(name.clone()),
            None => added.push(name),
        }
    }
    let dropped: Vec<&str> = GOLDEN
        .iter()
        .map(|(n, _, _)| *n)
        .filter(|n| !actual.iter().any(|(a, _, _)| a == n))
        .collect();

    if !moved.is_empty() || !added.is_empty() || !dropped.is_empty() {
        let mut what = Vec::new();
        if !moved.is_empty() {
            what.push(format!("{} moved: {}", moved.len(), moved.join(", ")));
        }
        if !added.is_empty() {
            what.push(format!("{} new: {}", added.len(), added.join(", ")));
        }
        if !dropped.is_empty() {
            what.push(format!("{} gone: {}", dropped.len(), dropped.join(", ")));
        }
        panic!(
            "canonical identities are not what GOLDEN records -- {}\n\n\
             A *moved* hash is not automatically a bug: `docs/canonical-ast.md` §8\n\
             says these are not frozen. Decide which happened, then act:\n\
             \x20 * you changed the encoding on purpose -> update GOLDEN below, and say\n\
             \x20   in the commit message that identities moved and why. That record is\n\
             \x20   what this test is for.\n\
             \x20 * you did not -> something reached the hash that should not have, and\n\
             \x20   this is the bug.\n\n\
             A *new* or *gone* row is only a fixture being added or removed.\n\n\
             The new table:\n\n{}",
            what.join("; "),
            render(&actual)
        );
    }
}

/// The table, ready to paste back into `GOLDEN`.
fn render(rows: &[(String, String, String)]) -> String {
    let mut out = String::from("const GOLDEN: &[(&str, &str, &str)] = &[\n");
    for (name, sig, body) in rows {
        out.push_str(&format!("    (\"{name}\", \"{sig}\", \"{body}\"),\n"));
    }
    out.push_str("];\n");
    out
}

/// Every fixture reaches a declaration, and no two fixtures are the same
/// program wearing different names.
///
/// Without this a fixture could quietly stop testing anything — a typo that
/// made two sources identical would still produce a stable table, and the
/// table would go on passing while covering one node kind less.
///
/// It compares the **pair**, not the body alone. Writing it the other way
/// fails immediately and correctly: `empty-row`, `reference` and
/// `unique-reference` have three different signatures over one body, because
/// all three bodies are `return 0;`. That is the whole point of hashing a
/// signature apart from a body, so the sameness is the feature.
#[test]
fn the_fixtures_are_distinct() {
    let mut seen: Vec<((String, String), &str)> = Vec::new();
    for (name, source, decl) in FIXTURES {
        let ast = parse(source).unwrap_or_else(|e| panic!("`{name}` should parse: {e:?}"));
        let ids = identify(&ast);
        let pair = match ids.function(decl) {
            Some(f) => (f.sig.to_hex(), f.body.to_hex()),
            None => {
                let t = ids
                    .type_decl(decl)
                    .unwrap_or_else(|| panic!("`{name}` should declare `{decl}`"));
                (t.id.to_hex(), t.id.to_hex())
            }
        };
        if let Some((_, other)) = seen.iter().find(|(p, _)| *p == pair) {
            panic!(
                "`{name}` and `{other}` hash the same; one of them is not testing what it names"
            );
        }
        seen.push((pair, name));
    }
}
