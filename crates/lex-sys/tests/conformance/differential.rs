//! The constant folder against the backend (`docs/differential.md`).

use super::*;

/// The integers every integer operator is tried on: both ends of the
/// range and one step in, the shift amount's edges on both sides, and the
/// small values every rule has a case for. Fourteen, so 196 pairs.
const DIFF_INTS: &[&str] = &[
    "-9223372036854775808",
    "-9223372036854775807",
    "-64",
    "-63",
    "-2",
    "-1",
    "0",
    "1",
    "2",
    "3",
    "63",
    "64",
    "9223372036854775806",
    "9223372036854775807",
];

/// The same for floats: both zeros, a value that is not exact in binary,
/// the largest finite magnitudes, the smallest subnormal and the smallest
/// normal, both infinities, and NaN with each sign.
const DIFF_FLOATS: &[&str] = &[
    "0.0",
    "-0.0",
    "1.0",
    "-1.0",
    "0.1",
    "3.0",
    "1.0e308",
    "-1.0e308",
    "5.0e-324",
    "2.2250738585072014e-308",
    "(1.0 / 0.0)",
    "(-1.0 / 0.0)",
    "(0.0 / 0.0)",
    "(-(0.0 / 0.0))",
];

const DIFF_BOOLS: &[&str] = &["false", "true"];

/// One expression the folder can evaluate: an operator, its operand type,
/// and one or two operands spelled as literals.
struct Case {
    ty: &'static str,
    ret: &'static str,
    op: &'static str,
    a: &'static str,
    b: Option<&'static str>,
}

impl Case {
    fn expr(&self, a: &str, b: &str) -> String {
        match self.op.strip_prefix('u') {
            Some(unary) => format!("{unary}({a})"),
            None => format!("({a}) {} ({b})", self.op),
        }
    }

    fn literal(&self) -> String {
        self.expr(self.a, self.b.unwrap_or(""))
    }

    /// The operator as a function of its operands, which is what the call
    /// arm folds through `evaluate_calls` rather than during lowering.
    fn helper(&self) -> String {
        let name = match self.op {
            "+" => "add",
            "-" => "sub",
            "*" => "mul",
            "/" => "div",
            "%" => "rem",
            "<<" => "shl",
            ">>" => "shr",
            "&" => "band",
            "|" => "bor",
            "^" => "bxor",
            "==" => "eq",
            "!=" => "ne",
            "<" => "lt",
            "<=" => "le",
            ">" => "gt",
            ">=" => "ge",
            "&&" => "land",
            "||" => "lor",
            "u-" => "neg",
            "u~" => "bnot",
            "u!" => "not",
            other => unreachable!("no helper for `{other}`"),
        };
        format!("{}_{name}", self.ty)
    }
}

fn differential_cases() -> Vec<Case> {
    let comparison = |op: &str| matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||");
    let mut cases = Vec::new();
    let mut binary = |ty: &'static str, ops: &[&'static str], values: &[&'static str]| {
        for &op in ops {
            let ret = if comparison(op) { "bool" } else { ty };
            for &a in values {
                for &b in values {
                    cases.push(Case { ty, ret, op, a, b: Some(b) });
                }
            }
        }
    };
    binary(
        "int",
        &["+", "-", "*", "/", "%", "<<", ">>", "&", "|", "^", "==", "!=", "<", "<=", ">", ">="],
        DIFF_INTS,
    );
    binary("float", &["+", "-", "*", "/", "==", "!=", "<", "<=", ">", ">="], DIFF_FLOATS);
    binary("bool", &["==", "!=", "&&", "||"], DIFF_BOOLS);
    for (ty, op, values) in
        [("int", "u-", DIFF_INTS), ("int", "u~", DIFF_INTS), ("float", "u-", DIFF_FLOATS)]
            .into_iter()
            .chain([("bool", "u!", DIFF_BOOLS)])
    {
        for &a in values {
            cases.push(Case { ty, ret: ty, op, a, b: None });
        }
    }
    cases
}

/// Parse `k v` lines, the one format both programs print.
fn numbered(text: &str) -> Vec<(usize, i64)> {
    text.lines()
        .map(|line| {
            let (k, v) = line.split_once(' ').expect("a `k v` line");
            (k.parse().expect("a case number"), v.parse().expect("a value"))
        })
        .collect()
}

const DIFF_SHARED: &str = "\
import std.io;
fn b2i(b: bool) -> [] int { if b { return 1; } return 0; }
";

/// `differential.md` §3.2: skip the core dump a trap would otherwise
/// cost.
///
/// A GitHub Linux runner pipes every crash to `systemd-coredump`, measured
/// there at 544 ms per trap and slowing as they pile up. For a piped
/// `core_pattern` the kernel ignores `RLIMIT_CORE` -- except the value
/// **1**, which it reserves to catch a crashing dump helper and answers
/// with *"RLIMIT_CORE is set to 1, aborting core"*. Measured with a
/// half-second helper: limit 0 costs 509 ms per trap, limit 1 costs 5.
/// Making the binary unreadable does not work there, because systemd also
/// sets `fs.suid_dumpable = 2`, which dumps non-dumpable processes too.
/// The trap is unchanged; only the dump goes.
#[cfg(target_os = "linux")]
fn without_a_core_dump(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    #[repr(C)]
    struct Rlimit {
        current: u64,
        maximum: u64,
    }
    unsafe extern "C" {
        fn setrlimit(resource: i32, limit: *const Rlimit) -> i32;
    }
    const RLIMIT_CORE: i32 = 4;
    // SAFETY: `setrlimit` is async-signal-safe, touches nothing but the
    // child's own limits, and allocates nothing, which is what `pre_exec`
    // requires of the closure it runs between `fork` and `exec`.
    unsafe {
        command.pre_exec(|| {
            let one = Rlimit { current: 1, maximum: 1 };
            if setrlimit(RLIMIT_CORE, &one) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        })
    }
}

#[cfg(not(target_os = "linux"))]
fn without_a_core_dump(command: &mut Command) -> &mut Command {
    command
}

/// C2(b) of the audit, and the test `fold.rs` had been citing for months
/// without it existing: **every operator the folder evaluates, on every
/// pair of boundary operands, gives the same answer folded as the
/// compiled program gives at run time** -- the same value, or a trap on
/// both sides.
///
/// Three arms per case, because the folder has two entrances:
///
/// - **literal**: `(a) op (b)` in a function of its own, folded during
///   lowering. A trap here is a `constant-traps` refusal, and `check`
///   reports every one, so one run of `check` is the folder's whole trap
///   set.
/// - **call**: `op_fn(a, b)`, a pure function on literal arguments,
///   folded by `evaluate_calls` after lowering. `authority` counts the
///   calls it folded, which is how this test knows the arm did not
///   silently fall through to run time.
/// - **run time**: the same operator on operands chosen by a loop counter
///   the compiler cannot see, so neither the folder nor Cranelift has a
///   constant to work with. A trap kills the process, so it reports on
///   standard error -- unbuffered, where standard output would lose
///   everything since the last flush -- and is restarted past the case
///   that killed it.
///
/// Measured against deliberately broken folders before it was trusted
/// (`differential.md` §3): each of five one-line mutations was caught.
#[test]
fn the_folder_agrees_with_the_backend() {
    let cases = differential_cases();
    let dir = scratch("differential");

    // ---- the folder's trap set: one function per case, on line k + 1 ----
    let literal: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(k, c)| format!("fn l{k}() -> [] {} {{ return {}; }}", c.ret, c.literal()))
        .collect();
    let probe = dir.join("literal.ls");
    std::fs::write(
        &probe,
        format!(
            "{}\nfn main(world: World) -> [] int {{ release(world); return 0; }}\n",
            literal.join("\n")
        ),
    )
    .expect("a writable fixture");
    let check = Command::new(BIN)
        .args(["check".as_ref(), probe.as_os_str(), "--output".as_ref(), "json".as_ref()])
        .output()
        .expect("the compiler runs");
    let report = String::from_utf8_lossy(&check.stdout);
    let refusals = report.matches("\"rule\":").count();
    assert_eq!(
        refusals,
        report.matches("\"rule\": \"constant-traps\"").count(),
        "a literal case was refused for something other than trapping:\n{report}"
    );
    let folder_traps: std::collections::BTreeSet<usize> = report
        .match_indices("\"line\": ")
        .map(|(at, key)| {
            let digits: String =
                report[at + key.len()..].chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<usize>().expect("a line number") - 1
        })
        .collect();
    assert_eq!(folder_traps.len(), refusals, "one refusal per trapping case");

    // ---- the two folded arms, for every case that has a value ----
    let mut helpers = std::collections::BTreeMap::new();
    for c in &cases {
        let body = match c.op.strip_prefix('u') {
            Some(unary) => {
                format!("fn {}(a: {}) -> [] {} {{ return {unary}a; }}", c.helper(), c.ty, c.ret)
            }
            None => format!(
                "fn {}(a: {ty}, b: {ty}) -> [] {} {{ return a {} b; }}",
                c.helper(),
                c.ret,
                c.op,
                ty = c.ty
            ),
        };
        helpers.insert(c.helper(), body);
    }
    let mut folded = String::from(DIFF_SHARED);
    folded.push_str(
        "fn out[&i](i: &!i Io, k: int, v: int) -> [io_write] int {\n    \
         io.print_int(i, k); io.space(i); io.print_int(i, v); io.newline(i); return 0;\n}\n",
    );
    for body in helpers.values() {
        folded.push_str(body);
        folded.push('\n');
    }
    let mut calls = String::new();
    for (k, c) in cases.iter().enumerate().filter(|(k, _)| !folder_traps.contains(k)) {
        let args = match c.b {
            Some(b) => format!("{}, {b}", c.a),
            None => c.a.to_owned(),
        };
        folded.push_str(&literal[k]);
        folded.push('\n');
        folded.push_str(&format!(
            "fn c{k}() -> [] {} {{ return {}({args}); }}\n",
            c.ret,
            c.helper()
        ));
        calls.push_str(&format!(
            "        out(i, {k}, {}); out(i, {k}, {});\n",
            shown(c.ret, &format!("l{k}()")),
            shown(c.ret, &format!("c{k}()"))
        ));
    }
    folded.push_str(&format!(
        "fn main(world: World) -> [] int {{\n    \
         let Split {{ io, ffi, fs, heap, args }} = split(world);\n    \
         release(args); release(heap); release(fs); release(ffi);\n    \
         borrow mut io as &!i in {{\n{calls}    }}\n    release(io);\n    return 0;\n}}\n"
    ));
    let folded_path = dir.join("folded.ls");
    std::fs::write(&folded_path, folded).expect("a writable fixture");

    let valued = cases.len() - folder_traps.len();
    let authority = Command::new(BIN)
        .args([
            "authority".as_ref(),
            folded_path.as_os_str(),
            "--std".as_ref(),
            "--output".as_ref(),
            "json".as_ref(),
        ])
        .output()
        .expect("the compiler runs");
    let authority = String::from_utf8_lossy(&authority.stdout);
    assert!(
        authority.contains(&format!("\"folded_calls\": {valued},")),
        "every call arm must fold, or the arm is run time against run time:\n{authority}"
    );

    let exe = dir.join("folded");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            folded_path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&exe).output().expect("the program runs");
    assert!(run.status.success(), "nothing in the folded program can trap");
    let mut at_compile_time = std::collections::BTreeMap::new();
    for (k, v) in numbered(&String::from_utf8_lossy(&run.stdout)) {
        if let Some(seen) = at_compile_time.insert(k, v) {
            assert_eq!(
                seen,
                v,
                "the two halves of the folder disagree on `{}` ({}): lowering says {seen}, \
                 `evaluate_calls` says {v}",
                cases[k].literal(),
                cases[k].ty
            );
        }
    }
    assert_eq!(at_compile_time.len(), valued);

    // ---- run time: operands the compiler cannot see ----
    let mut runtime = String::from(DIFF_SHARED);
    runtime.push_str(
        "\
fn digits[&o](buf: &!o [byte], at: int, n: int) -> [] int {
    var p = at;
    var m = n;
    if m > 0 { m = -m; }
    if m == 0 { p = p - 1; buf[p] = byte_of('0'); }
    while m != 0 {
        p = p - 1;
        buf[p] = byte_of('0' - m % 10);
        m = m / 10;
    }
    if n < 0 { p = p - 1; buf[p] = byte_of('-'); }
    return p;
}
fn out[&i](i: &!i Io, k: int, v: int) -> [err_write] int {
    region r {
        let buf = alloc_slice[r](48, byte_of(0));
        buf[47] = byte_of('\\n');
        var p = digits(buf, 47, v);
        p = p - 1;
        buf[p] = byte_of(' ');
        p = digits(buf, p, k);
        io.error_all(i, buf[p..48]);
    }
    return 0;
}
fn first[&a](a: &a Args) -> [args] int {
    let s = arg(a, 1);
    var n = 0;
    var p = 0;
    while p < len(s) { n = n * 10 + (int_of(s[p]) - '0'); p = p + 1; }
    return n;
}
",
    );
    for (name, ty, values) in
        [("ival", "int", DIFF_INTS), ("fval", "float", DIFF_FLOATS), ("bval", "bool", DIFF_BOOLS)]
    {
        runtime.push_str(&format!("fn {name}(k: int) -> [] {ty} {{\n"));
        for (i, v) in values.iter().enumerate() {
            runtime.push_str(&format!("    if k == {i} {{ return {v}; }}\n"));
        }
        runtime.push_str(&format!("    return {};\n}}\n", values[0]));
    }
    // One section per operator, in the order `differential_cases` made
    // them, decoding the case number back into two operand indices.
    runtime.push_str("fn case(k: int) -> [] int {\n");
    let mut start = 0;
    while start < cases.len() {
        let c = &cases[start];
        let count = cases[start..].iter().take_while(|d| d.ty == c.ty && d.op == c.op).count();
        let (getter, n) = match c.ty {
            "int" => ("ival", DIFF_INTS.len()),
            "float" => ("fval", DIFF_FLOATS.len()),
            _ => ("bval", DIFF_BOOLS.len()),
        };
        let e = match c.b {
            None => c.expr(&format!("{getter}(j)"), ""),
            Some(_) => c.expr(&format!("{getter}(j / {n})"), &format!("{getter}(j % {n})")),
        };
        runtime.push_str(&format!(
            "    if k < {} {{\n        let j = k - {start};\n        return {};\n    }}\n",
            start + count,
            shown(c.ret, &e)
        ));
        start += count;
    }
    runtime.push_str(&format!(
        "    return 0;\n}}\n\
         fn main(world: World) -> [] int {{\n    \
         let Split {{ io, ffi, fs, heap, args }} = split(world);\n    \
         release(heap); release(fs); release(ffi);\n    \
         var k = 0;\n    \
         borrow args as &a in {{ k = first(a); }}\n    \
         release(args);\n    \
         borrow mut io as &!i in {{\n        \
         while k < {} {{ out(i, k, case(k)); k = k + 1; }}\n    }}\n    \
         release(io);\n    return 0;\n}}\n",
        cases.len()
    ));
    let runtime_path = dir.join("runtime.ls");
    std::fs::write(&runtime_path, runtime).expect("a writable fixture");
    let exe = dir.join("runtime");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            "--std".as_ref(),
            runtime_path.as_os_str(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // Every trap is a process the kernel kills, and each one costs
    // whatever the host does with a crash -- a core dump, or a handler
    // `core_pattern` pipes it to. So the loop is bounded three ways and
    // says what it saw when it stops: a run that neither finishes nor
    // traps is killed, a run of traps the folder did not predict stops
    // early rather than paying for one crash per remaining case, and the
    // whole phase has a budget.
    let started = std::time::Instant::now();
    let mut at_run_time = std::collections::BTreeMap::new();
    let mut runtime_traps = std::collections::BTreeSet::new();
    let mut unpredicted = Vec::new();
    let mut runs = 0;
    let mut slowest = (std::time::Duration::ZERO, 0, false);
    let mut trap_time = std::time::Duration::ZERO;
    let report = |what: &str,
                  runs: usize,
                  traps: usize,
                  slowest: (std::time::Duration, usize, bool),
                  trap_time: std::time::Duration| {
        let pattern = std::fs::read_to_string("/proc/sys/kernel/core_pattern")
            .map(|p| p.trim().to_owned())
            .unwrap_or_else(|_| "unreadable".to_owned());
        format!(
            "{what}: {runs} runs, {traps} traps, {:.1?} elapsed; slowest run {:.1?} \
             (from case {}, {}); {:.1?} per trapping run; core_pattern `{pattern}`",
            started.elapsed(),
            slowest.0,
            slowest.1,
            if slowest.2 { "trapped" } else { "finished" },
            trap_time / u32::try_from(traps.max(1)).unwrap_or(1),
        )
    };
    let mut next = 0;
    while next < cases.len() {
        let began = std::time::Instant::now();
        let mut child = without_a_core_dump(Command::new(&exe).arg(next.to_string()))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the program runs");
        let mut pipe = child.stderr.take().expect("a piped standard error");
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
            text
        });
        let status = loop {
            if let Some(status) = child.try_wait().expect("the program can be waited on") {
                break status;
            }
            if began.elapsed() > std::time::Duration::from_secs(30) {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "{}",
                    report(
                        &format!("a run from case {next} neither finished nor trapped in 30 s"),
                        runs + 1,
                        runtime_traps.len(),
                        slowest,
                        trap_time
                    )
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let stderr = reader.join().expect("the reader thread finishes");
        runs += 1;
        let took = began.elapsed();
        let mut reached = next;
        for (k, v) in numbered(&stderr) {
            at_run_time.insert(k, v);
            reached = k + 1;
        }
        if took > slowest.0 {
            slowest = (took, next, !status.success());
        }
        if status.success() {
            assert_eq!(reached, cases.len(), "a clean exit must have run every case");
            break;
        }
        // A trap is a signal, never an exit code (`defined-behaviour.md`
        // §1): an exit status here would be a different failure.
        assert_eq!(status.code(), None, "case {reached} ended without a signal");
        trap_time += took;
        runtime_traps.insert(reached);
        if !folder_traps.contains(&reached) {
            unpredicted.push(format!("`{}` ({})", cases[reached].literal(), cases[reached].ty));
            assert!(
                unpredicted.len() <= 20,
                "{}; traps the folder did not predict, first 20:\n{}",
                report("stopped early", runs, runtime_traps.len(), slowest, trap_time),
                unpredicted.join("\n")
            );
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(300),
            "{}",
            report("over the 300 s budget", runs, runtime_traps.len(), slowest, trap_time)
        );
        next = reached + 1;
    }

    // ---- the comparison ----
    let mut disagreements = Vec::new();
    for (k, c) in cases.iter().enumerate() {
        let what = format!("`{}` ({})", c.literal(), c.ty);
        match (folder_traps.contains(&k), runtime_traps.contains(&k)) {
            (true, false) => disagreements.push(format!(
                "{what}: the folder refuses it as a trap, the program answers {}",
                at_run_time[&k]
            )),
            (false, true) => disagreements.push(format!(
                "{what}: the folder answers {}, the program traps",
                at_compile_time[&k]
            )),
            (false, false) if at_compile_time[&k] != at_run_time[&k] => disagreements.push(
                format!("{what}: folded {}, at run time {}", at_compile_time[&k], at_run_time[&k]),
            ),
            _ => {}
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} cases disagree:\n{}",
        disagreements.len(),
        cases.len(),
        disagreements.join("\n")
    );
    // The sizes `differential.md` §3 reports, so a change to the operand
    // tables or the operator set is a change to the document too.
    assert_eq!((cases.len(), folder_traps.len()), (5156, 436));
    let _ = std::fs::remove_dir_all(&dir);
}
