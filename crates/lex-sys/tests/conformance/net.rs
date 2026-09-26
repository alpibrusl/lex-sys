//! The network: `examples/serve/`, `examples/fetch/`, and what the reports say about them.

use super::*;

/// `examples/serve/` — a REST endpoint, answered over a real TCP socket.
///
/// This is the test `docs/reach.md` rests on. The document's claim is that
/// a lex-sys program can serve HTTP **today**, with no socket type, no
/// `Net` capability and no library, and a claim like that is worth exactly
/// what it is tested with. So the test is not "it compiles": it builds the
/// program, runs it, connects to it from this process over loopback, and
/// reads what comes back.
///
/// It runs twice because the server answers one request and exits, which
/// is what makes it testable without a shutdown protocol — the second run
/// is the 404, and the two together are the router.
///
/// The program takes its port from `argv`, so this picks one the operating
/// system says is free rather than hard-coding a number that CI might
/// already be using.
#[test]
fn an_http_server_written_in_lex_sys_answers_a_real_request() {
    use std::io::{Read, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    let scratch = scratch("example-serve");
    let exe = scratch.join("serve");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            repo_root().join("examples/serve/serve.ls").as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(
        build.status.success(),
        "`serve` should compile, but the compiler said:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    // `[(request target, status line, body)]` — one exchange per run.
    let exchanges = [
        ("/health", "HTTP/1.1 200 OK", "{\"ok\":true}"),
        ("/nothing-here", "HTTP/1.1 404 Not Found", "{\"error\":\"not found\"}"),
    ];

    for (target, status_line, body) in exchanges {
        // Bind and drop: the port is free at this instant, which is the
        // best any test can say about a port it did not get from the
        // program itself.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("a free loopback port")
            .local_addr()
            .expect("a bound address")
            .port();

        let mut child = Command::new(&exe)
            .arg(port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiled server runs");

        // The server is a process, so "listening" is not an event this
        // test can observe — it retries the connection until the listen
        // backlog exists or the deadline passes.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => break s,
                Err(e) if Instant::now() < deadline => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    let _ = child.kill();
                    panic!("the server never accepted a connection on {port}: {e}");
                }
            }
        };

        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a readable socket");
        write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("the server accepts a request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("the server answers");

        assert!(
            response.starts_with(status_line),
            "`serve` answered `{target}` with the wrong status:\n{response}"
        );
        assert!(
            response.ends_with(&format!("\r\n\r\n{body}")),
            "`serve` answered `{target}` with the wrong body:\n{response}"
        );
        // The length it declares and the bytes it sends come from the same
        // slice, and this is that being true rather than assumed.
        assert!(
            response.contains(&format!("Content-Length: {}\r\n", body.len())),
            "`serve` declared a length that is not the body's:\n{response}"
        );

        let run = child.wait_with_output().expect("the server exits");
        assert_eq!(run.status.code(), Some(0), "`serve` exited wrongly after one request");
    }

    let _ = std::fs::remove_dir_all(&scratch);
}

/// And what the authority report says about it, which is §5's whole point.
///
/// The row is `ffi("libc")` and nothing else: exact, and silent about the
/// network, because a library is not an authority domain. What covers the
/// difference is the foreign symbol list — `socket`, `bind`, `listen`,
/// `accept` are in the binary because `main` reaches them, and a
/// supervisor reading that list knows what it is being asked to run.
#[test]
fn the_authority_report_names_the_syscalls_the_row_cannot() {
    let output = Command::new(BIN)
        .args(["authority".as_ref(), repo_root().join("examples/serve/serve.ls").as_os_str()])
        .args(["--std", "--output", "json"])
        .output()
        .expect("the compiler runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = String::from_utf8(output.stdout).expect("the report is utf-8");

    // Two capabilities, and it proves the absence of the other three.
    assert!(report.contains("\"effects\": [\"args\", \"ffi\"]"), "{report}");
    assert!(
        report.contains("{ \"name\": \"ffi\", \"argument\": \"libc\", \"bounded\": false }"),
        "{report}"
    );

    // The row says "calls a C library". The symbols say which library
    // calls, and those are the ones that name a socket.
    for symbol in ["socket", "bind", "listen", "accept"] {
        assert!(
            report.contains(&format!("\"{symbol}\"")),
            "the report should name `{symbol}`:\n{report}"
        );
    }
}

/// `docs/net.md` §5 — the network programs, counted.
///
/// §5 counted the askers for each half of the network and found inbound
/// 1, outbound 0, and this test used to be `the_only_network_program_is_inbound`:
/// written to **fail** the day a program that connects arrived, so the
/// count could not quietly age. `examples/fetch/` arrived, it failed, and
/// §5 was rewritten (`docs/connect.md`). It is a count now, for the same
/// reason: the bar for building `Net` is two askers per half, and whoever
/// adds the next network program should have to change a number here and
/// the sentence in §5 that rests on it.
///
/// `examples/report/` recounted outbound to 2 (`docs/connect.md` §6).
/// `examples/collect/` does the same for inbound (`docs/listen.md`
/// §4): both halves have now cleared the bar. `examples/vsock/`
/// recounted outbound to 3 (`docs/net.md` §5's own note on it), and
/// `examples/agent_guest/`/`examples/agent_supervisor/` -- the
/// guest/supervisor exchange over plain HTTP, `docs/net.md`'s own
/// entry on them -- recount outbound to 4 and inbound to 3.
#[test]
fn the_network_programs_are_counted() {
    let root = repo_root();
    let mut inbound = std::collections::BTreeSet::new();
    let mut outbound = std::collections::BTreeSet::new();

    let mut sources: Vec<PathBuf> = Vec::new();
    for directory in ["examples", "std", "tests/accept"] {
        let mut stack = vec![root.join(directory)];
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at).expect("a readable directory") {
                let path = entry.expect("a readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "ls") {
                    sources.push(path);
                }
            }
        }
    }

    for path in &sources {
        let text = std::fs::read_to_string(path).expect("a readable program");
        let relative = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("extern fn ") else { continue };
            let Some(name) = rest.split(['[', '(']).next() else { continue };
            let name = name.trim();
            if ["bind", "listen", "accept"].contains(&name) {
                inbound.insert(relative.clone());
            }
            if ["connect", "sendto", "getaddrinfo"].contains(&name) {
                outbound.insert(relative.clone());
            }
        }
    }

    assert_eq!(
        (
            inbound.iter().map(String::as_str).collect::<Vec<_>>(),
            outbound.iter().map(String::as_str).collect::<Vec<_>>()
        ),
        (
            vec![
                "examples/agent_supervisor/agent_supervisor.ls",
                "examples/collect/collect.ls",
                "examples/serve/serve.ls"
            ],
            vec![
                "examples/agent_guest/agent_guest.ls",
                "examples/fetch/fetch.ls",
                "examples/report/report.ls",
                "examples/vsock/vsock.ls"
            ]
        ),
        "the network programs changed: `net.md` §5 counts inbound 3, outbound 4, and \
         two is the bar for building `Net`. Rewrite §5, then this."
    );
}

// ---------------------------------------------------------------------
// `examples/fetch/` — the first program that connects (`docs/connect.md`)
// ---------------------------------------------------------------------

/// A lex-sys client, fetching from a lex-sys server.
///
/// Both halves of the network in one test: `examples/serve/` binds and
/// accepts, `examples/fetch/` connects, and neither is a Rust stand-in.
/// `fetch` exits 3 when nothing is listening yet, so it is retried until
/// the server is up -- the same "listening is not an event" problem the
/// server's own test has, seen from the other side.
#[test]
fn a_lex_sys_client_fetches_from_a_lex_sys_server() {
    use std::time::{Duration, Instant};
    let (server_dir, server) = build_example("fetch-server", "examples/serve/serve.ls", "serve");
    let (client_dir, client) = build_example("fetch-client", "examples/fetch/fetch.ls", "fetch");

    // `(path, body, exit status)` -- one exchange per server run, because
    // the server answers once and exits.
    let exchanges =
        [("/health", "{\"ok\":true}", 0), ("/elsewhere", "{\"error\":\"not found\"}", 1)];
    for (path, body, status) in exchanges {
        let port = free_port().to_string();
        let mut child = Command::new(&server)
            .arg(&port)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server runs");
        let deadline = Instant::now() + Duration::from_secs(10);
        let run = loop {
            let run = Command::new(&client)
                .args(["127.0.0.1", port.as_str(), path])
                .output()
                .expect("the client runs");
            if run.status.code() != Some(3) || Instant::now() > deadline {
                break run;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let _ = child.wait();
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            body,
            "`fetch {path}` should print exactly the body; stderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.status.code(), Some(status), "`fetch {path}` exited wrongly");
    }
    let _ = std::fs::remove_dir_all(&server_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// What `fetch` sends, byte for byte, and that it survives a server that
/// splits its header block across writes and sends more body than either
/// of its 4 KiB buffers holds.
///
/// The header block's end is searched for over everything received so
/// far, so a `\r\n\r\n` that straddles two reads is still found; and
/// after it every byte goes straight to standard output. A 100,000-byte
/// body is 25 of the client's reads.
#[test]
fn fetch_speaks_http_1_0_and_streams_the_body() {
    use std::io::{Read, Write as _};
    use std::time::Duration;
    let (dir, client) = build_example("fetch-wire", "examples/fetch/fetch.ls", "fetch");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let port = listener.local_addr().expect("a bound address").port().to_string();
    let body: Vec<u8> = (0..100_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let served = body.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the client connects");
        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a timeout");
        let mut request = Vec::new();
        let mut chunk = [0u8; 512];
        while !request.ends_with(b"\r\n\r\n") {
            let n = stream.read(&mut chunk).expect("the request arrives");
            assert!(n > 0, "the client closed before finishing its request");
            request.extend_from_slice(&chunk[..n]);
        }
        // Split inside the blank line, so the terminator straddles reads.
        stream.write_all(b"HTTP/1.0 200 OK\r\nX-Split: yes\r\n\r").expect("a write");
        stream.flush().expect("a flush");
        std::thread::sleep(Duration::from_millis(50));
        stream.write_all(b"\n").expect("a write");
        stream.write_all(&served).expect("the body");
        request
    });
    let run = Command::new(&client)
        .args(["127.0.0.1", port.as_str(), "/a/b?c=d"])
        .output()
        .expect("the client runs");
    let request = server.join().expect("the server thread finishes");
    assert_eq!(
        String::from_utf8_lossy(&request),
        "GET /a/b?c=d HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(run.stdout == body, "the body came back as {} bytes, not 100000", run.stdout.len());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/connect.md` §1 — a name is refused, because nothing here can
/// resolve one, and the refusal says so rather than failing to connect.
#[test]
fn fetch_refuses_a_name_it_cannot_resolve() {
    let (dir, client) = build_example("fetch-names", "examples/fetch/fetch.ls", "fetch");
    for (args, message) in [
        (["localhost", "80", "/"], "there is no name resolution"),
        (["10.0.0", "80", "/"], "four decimal octets"),
        (["10.0.0.256", "80", "/"], "four decimal octets"),
        (["10.0.0.1", "0", "/"], "1..65535"),
        (["10.0.0.1", "65536", "/"], "1..65535"),
    ] {
        let run = Command::new(&client).args(args).output().expect("the client runs");
        assert_eq!(run.status.code(), Some(2), "`fetch {args:?}` should be a usage error");
        assert!(
            String::from_utf8_lossy(&run.stderr).contains(message),
            "`fetch {args:?}` should say `{message}`:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/connect.md` §3 — `struct sockaddr_in` is two different byte
/// arrays, and the Linux one is accepted on both targets.
///
/// Linux starts it with a two-byte family, `2, 0`; macOS with a length
/// byte and a one-byte family, `16, 2`. `examples/serve/` writes the Linux
/// bytes and binds on macOS anyway, so the question was whether `connect`
/// is as forgiving. This probe connects with one layout at a time to a
/// listener in this process and asserts the answer for the platform it
/// runs on: each CI runner checks its own row.
///
/// The first version of this test asserted that macOS **refuses** `2, 0`,
/// and the darwin-aarch64 runner said otherwise: BSD reads family 0 as
/// `AF_INET` in `connect` as well as in `bind`. That is what the
/// assertions below now pin, and what `connect.md` §3 was corrected to.
#[test]
fn the_linux_address_layout_connects_on_both_targets() {
    let source = "\
extern fn socket[&f](ffi: &f Ffi(\"libc\"), domain: int, kind: int, proto: int)
    -> [ffi(\"libc\")] c_int;
extern fn connect[&f, &a](ffi: &f Ffi(\"libc\"), fd: int, addr: &a [byte])
    -> [ffi(\"libc\")] c_int;
fn main(world: World) -> [] int {
    let Split { io, ffi, fs, heap, args } = split(world);
    release(io); release(fs); release(heap);
    let libc = narrow(ffi, \"libc\");
    var status = 2;
    borrow libc as &f in {
        borrow args as &g in {
            let text = arg(g, 1);
            var port = 0;
            var i = 0;
            while i < len(text) { port = port * 10 + (int_of(text[i]) - '0'); i = i + 1; }
            region r {
                let addr = alloc_slice[r](16, byte_of(0));
                if int_of(arg(g, 2)[0]) == 'b' {
                    addr[0] = byte_of(16);
                    addr[1] = byte_of(2);
                } else {
                    addr[0] = byte_of(2);
                }
                addr[2] = byte_of(port / 256);
                addr[3] = byte_of(port % 256);
                addr[4] = byte_of(127);
                addr[7] = byte_of(1);
                let fd = socket(f, 2, 1, 0);
                status = 1;
                if connect(f, fd, addr) == 0 { status = 0; }
            }
        }
    }
    release(libc);
    release(args);
    return status;
}
";
    let dir = scratch("address-layout");
    let path = dir.join("layout.ls");
    std::fs::write(&path, source).expect("a writable fixture");
    let exe = dir.join("layout");
    let build = Command::new(BIN)
        .args(["build".as_ref(), path.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    // A listener that is never accepted from: the kernel completes the
    // handshake into the backlog, which is all `connect` waits for.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let port = listener.local_addr().expect("a bound address").port().to_string();
    let connects = |layout: &str| {
        Command::new(&exe).args([port.as_str(), layout]).status().expect("the probe runs").code()
            == Some(0)
    };
    let (linux, bsd) = (connects("linux"), connects("bsd"));
    if cfg!(target_os = "linux") {
        assert!(linux, "Linux should accept its own layout, `2, 0`");
        assert!(!bsd, "Linux should refuse `16, 2`, which it reads as family 528");
    }
    if cfg!(target_os = "macos") {
        assert!(bsd, "macOS should accept its own layout, `16, 2`");
        assert!(linux, "macOS should accept `2, 0` too, reading family 0 as `AF_INET`");
    }
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the authority report says about a program that connects: the
/// same unbounded `ffi("libc")` as the server, and a symbol list that
/// tells the two apart -- `connect` here, `bind`/`listen`/`accept` there.
#[test]
fn the_client_and_the_server_differ_only_in_their_symbols() {
    let (client_effects, client_symbols, _) = authority_of("examples/fetch/fetch.ls");
    let (server_effects, server_symbols, _) = authority_of("examples/serve/serve.ls");
    assert!(client_effects.contains(&"ffi".to_owned()), "{client_effects:?}");
    assert!(server_effects.contains(&"ffi".to_owned()), "{server_effects:?}");
    assert!(client_symbols.contains(&"connect".to_owned()), "{client_symbols:?}");
    for inbound in ["bind", "listen", "accept"] {
        assert!(!client_symbols.contains(&inbound.to_owned()), "{client_symbols:?}");
        assert!(server_symbols.contains(&inbound.to_owned()), "{server_symbols:?}");
    }
    assert!(!server_symbols.contains(&"connect".to_owned()), "{server_symbols:?}");
}

// ---------------------------------------------------------------------
// `examples/report/` — the second outbound program (`docs/connect.md` §6, #171)
// ---------------------------------------------------------------------

/// A lex-sys agent, posting to a lex-sys server.
///
/// `serve/`'s router only matches `GET /health`; everything else,
/// including a `POST`, gets its 404 default. That is enough to prove
/// `report` speaks real HTTP over a real connection to another lex-sys
/// program, the same way `a_lex_sys_client_fetches_from_a_lex_sys_server`
/// does for `fetch` -- without needing `serve/` to grow a route it has
/// no asker for.
#[test]
fn a_lex_sys_agent_reports_to_a_lex_sys_server() {
    use std::time::{Duration, Instant};
    let (server_dir, server) = build_example("report-server", "examples/serve/serve.ls", "serve");
    let (client_dir, client) =
        build_example("report-client", "examples/report/report.ls", "report");

    let port = free_port().to_string();
    let mut child = Command::new(&server)
        .arg(&port)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server runs");
    let deadline = Instant::now() + Duration::from_secs(10);
    let run = loop {
        let run = Command::new(&client)
            .args(["127.0.0.1", port.as_str(), "/result", "42"])
            .output()
            .expect("the client runs");
        if run.status.code() != Some(3) || Instant::now() > deadline {
            break run;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = child.wait();
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "{\"error\":\"not found\"}",
        "`report` should print exactly the body; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(1), "`report` exited wrongly for a 404");
    let _ = std::fs::remove_dir_all(&server_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// What `report` sends, byte for byte -- a `POST` with a `Content-Length`
/// matching the body that follows it -- and that a body larger than the
/// client's own 4 KiB buffers still arrives whole, which takes more than
/// one `write` on the socket.
#[test]
fn report_sends_a_body_the_server_can_read_in_full() {
    use std::io::{Read, Write as _};
    use std::time::Duration;
    let (dir, client) = build_example("report-wire", "examples/report/report.ls", "report");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let port = listener.local_addr().expect("a bound address").port().to_string();
    let message: String = (0..100_000u32).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the client connects");
        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a timeout");
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).expect("the request arrives");
            assert!(n > 0, "the client closed before finishing its request");
            request.extend_from_slice(&chunk[..n]);
            let Some(header_end) = find_double_crlf(&request) else { continue };
            let content_length = header_of(&request[..header_end], "Content-Length")
                .expect("a Content-Length header")
                .parse::<usize>()
                .expect("a numeric Content-Length");
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok").expect("a write");
        request
    });
    let run = Command::new(&client)
        .args(["127.0.0.1", port.as_str(), "/result", &message])
        .output()
        .expect("the client runs");
    let request = server.join().expect("the server thread finishes");
    let header_end = find_double_crlf(&request).expect("a complete header block");
    assert_eq!(
        &request[..header_end],
        format!(
            "POST /result HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close",
            message.len()
        )
        .as_bytes()
    );
    assert_eq!(&request[header_end + 4..], message.as_bytes(), "the body did not arrive whole");
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(run.stdout, b"ok", "the client should print the response body");
    let _ = std::fs::remove_dir_all(&dir);
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// The value of one header in a raw HTTP header block, case-sensitive --
/// enough for a test that wrote the request itself and knows its casing.
fn header_of<'a>(head: &'a [u8], name: &str) -> Option<&'a str> {
    let head = std::str::from_utf8(head).ok()?;
    for line in head.split("\r\n") {
        if let Some(rest) = line.strip_prefix(name).and_then(|r| r.strip_prefix(": ")) {
            return Some(rest);
        }
    }
    None
}

// ---------------------------------------------------------------------
// `examples/collect/` -- the second inbound program (`docs/listen.md`, #176)
// ---------------------------------------------------------------------

/// Connect, retrying only the connect itself -- while `collect` has not
/// called `listen` yet, refused -- and reuse that same connection for
/// the request, rather than a throwaway probe connection and a second
/// real one. `collect` accepts exactly as many connections as its
/// `count`, so a probe that connects and drops would be accepted and
/// counted as a request that never sent one, the same "listening is
/// not an event" problem `examples/serve/`'s own test has, just costed
/// wrong if solved with a separate connection.
fn post_when_ready(port: u16, deadline: std::time::Instant, path: &str, body: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write as _};
    use std::time::Duration;
    let mut stream = loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("the server never started listening on {port}: {e}"),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(10))).expect("a readable socket");
    write!(
        stream,
        "POST {path} HTTP/1.0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("the header sends");
    stream.write_all(body).expect("the body sends");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("the server answers");
    response
}

/// `collect` accepts more than one connection without restarting, and
/// reads each request's body in full -- the two things `examples/serve/`
/// never had to do (`docs/listen.md` §1).
#[test]
fn an_inbound_agent_reads_several_requests_in_a_row() {
    use std::time::{Duration, Instant};
    let (dir, exe) = build_example("collect-several", "examples/collect/collect.ls", "collect");
    let port = free_port();
    let child = Command::new(&exe)
        .args([port.to_string(), "3".to_owned()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server runs");

    let deadline = Instant::now() + Duration::from_secs(10);
    for (path, body) in [("/a", &b"one"[..]), ("/b", b"two-longer-message"), ("/c", b"three")] {
        let response = post_when_ready(port, deadline, path, body);
        assert!(
            response.starts_with(b"HTTP/1.1 200 OK"),
            "`collect {path}` should answer 200:\n{}",
            String::from_utf8_lossy(&response)
        );
    }

    let run = child.wait_with_output().expect("the server exits");
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(
        run.stdout, b"onetwo-longer-messagethree",
        "the three bodies should print in order with nothing between them"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A body larger than `collect`'s own 4 KiB read buffer, which every
/// body over that size takes more than one `read` to receive no matter
/// how the client wrote it -- `docs/listen.md` §2, and the inbound
/// mirror of `report_sends_a_body_the_server_can_read_in_full`.
///
/// `collect` writes the body to its own standard output as it reads it
/// (`docs/listen.md` §2 -- streamed, not materialised, the same reason
/// `examples/report/`'s bug in `docs/connect.md` §8 does not repeat
/// here). A pipe's kernel buffer is smaller than 100,000 bytes, so a
/// child whose stdout nothing drains blocks the moment it fills --
/// which is a deadlock in a test that waits for the HTTP exchange to
/// finish before reading that pipe, not a bug in `collect` itself: a
/// version of this test that read `child`'s stdout only after `post`
/// returned hung on exactly this. Draining it on its own thread, at
/// the same time as the exchange, is what a real reader of a large
/// response already does.
#[test]
fn collect_reads_a_body_larger_than_one_read() {
    use std::io::Read as _;
    use std::time::{Duration, Instant};
    let (dir, exe) = build_example("collect-large", "examples/collect/collect.ls", "collect");
    let port = free_port();
    let mut child = Command::new(&exe)
        .args([port.to_string(), "1".to_owned()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server runs");

    let mut stdout_pipe = child.stdout.take().expect("a piped stdout");
    let drain = std::thread::spawn(move || {
        let mut collected = Vec::new();
        stdout_pipe.read_to_end(&mut collected).expect("the pipe reads");
        collected
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let body: Vec<u8> = (0..100_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let response = post_when_ready(port, deadline, "/big", &body);
    assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{}", String::from_utf8_lossy(&response));

    let collected = drain.join().expect("the drain thread finishes");
    let run = child.wait().expect("the server exits");
    assert_eq!(run.code(), Some(0));
    assert_eq!(collected, body, "the body should arrive whole and in order");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/connect.md` §10.1: `connect` checks the dialled host against the
/// capability's bound *before* resolving anything, the same order
/// `a_path_outside_the_granted_prefix_traps` pins for `Fs`.
#[test]
fn connecting_outside_the_granted_host_traps() {
    let dir = scratch("net-outside-host");
    let source = dir.join("outside_host.ls");
    std::fs::write(
        &source,
        "edition 2;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args, net } = split(world);\n\
             release(args); release(heap); release(ffi); release(fs); release(io);\n\
             let bound = narrow(net, \"127.0.0.1:1\");\n\
             var fd = 0;\n\
             borrow bound as &n in {\n\
                 fd = connect(n, \"10.0.0.1\", 1);\n\
             }\n\
             release(bound);\n\
             return fd;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("outside_host");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a host outside the bound should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

/// §10.1: the bound's port half is checked too, exactly, not as a prefix
/// -- `"127.0.0.1:1"` authorises port 1 and no other.
#[test]
fn connecting_to_the_wrong_port_traps() {
    let dir = scratch("net-wrong-port");
    let source = dir.join("wrong_port.ls");
    std::fs::write(
        &source,
        "edition 2;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args, net } = split(world);\n\
             release(args); release(heap); release(ffi); release(fs); release(io);\n\
             let bound = narrow(net, \"127.0.0.1:1\");\n\
             var fd = 0;\n\
             borrow bound as &n in {\n\
                 fd = connect(n, \"127.0.0.1\", 2);\n\
             }\n\
             release(bound);\n\
             return fd;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("wrong_port");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a port outside the bound should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `docs/listen.md` §6: `bind`, `listen` and `accept`, built, answering a
/// real client over loopback -- the inbound mirror of
/// `a_lex_sys_client_fetches_from_a_lex_sys_server`, but reaching `Net`
/// directly rather than `examples/serve/`'s hand-rolled `extern fn`s.
#[test]
fn a_lex_sys_listener_accepts_a_real_connection() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let dir = scratch("net-bind-accept");
    let port = free_port();
    let source = dir.join("listener.ls");
    std::fs::write(
        &source,
        format!(
            "edition 2;\n\
             // `close` is deliberately not declared here: this compiler's\n\
             // `extern fn` convention crosses every `int` as 64 bits\n\
             // (`docs/reach.md` §3), while `bind`'s own internal `close`\n\
             // (the failure path, unused on this one) is declared against\n\
             // libc's true 32-bit `int` -- two different signatures for\n\
             // one linker symbol, which Cranelift correctly refuses. A\n\
             // short-lived test process needs no explicit close: the OS\n\
             // reclaims both descriptors when it exits.\n\
             extern fn read[&f, &b](ffi: &f Ffi(\"libc\"), fd: int, buf: &!b [byte]) -> [ffi(\"libc\")] int;\n\
             extern fn write[&f, &b](ffi: &f Ffi(\"libc\"), fd: int, buf: &b [byte]) -> [ffi(\"libc\")] int;\n\
             fn main(world: World) -> [] int {{\n\
                 let Split {{ io, ffi, fs, heap, args, net }} = split(world);\n\
                 release(io); release(fs); release(heap); release(args);\n\
                 let libc = narrow(ffi, \"libc\");\n\
                 let bound = narrow(net, \"{port}\");\n\
                 var status = 1;\n\
                 borrow bound as &n in {{\n\
                     borrow libc as &f in {{\n\
                         let listener = bind(n, {port});\n\
                         if listener >= 0 {{\n\
                             listen(listener, 1);\n\
                             let conn = accept(listener);\n\
                             if conn >= 0 {{\n\
                                 region scratch {{\n\
                                     let buf = alloc_slice[scratch](64, byte_of(0));\n\
                                     let got = read(f, conn, buf);\n\
                                     if got > 0 {{\n\
                                         write(f, conn, buf[0..got]);\n\
                                         status = 0;\n\
                                     }}\n\
                                 }}\n\
                             }}\n\
                         }}\n\
                     }}\n\
                 }}\n\
                 release(libc);\n\
                 release(bound);\n\
                 return status;\n\
             }}\n",
        ),
    )
    .expect("a writable fixture");

    let exe = dir.join("listener");
    let build = Command::new(BIN)
        .args([
            "build".as_ref(),
            source.as_os_str(),
            "--std".as_ref(),
            "-o".as_ref(),
            exe.as_os_str(),
        ])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let mut child = Command::new(&exe).spawn().expect("the listener runs");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("could not connect within the deadline: {e}"),
        }
    };
    stream.write_all(b"ping").expect("the write succeeds");
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).expect("the read succeeds");
    assert_eq!(&echoed, b"ping", "the accepted connection should echo what was sent");
    drop(stream);

    let run = child.wait().expect("the listener exits");
    assert_eq!(run.code(), Some(0), "the listener should report success");
    let _ = std::fs::remove_dir_all(&dir);
}

/// §6.1: `bind`'s port is checked against the capability's bound, the
/// inbound mirror of `connecting_to_the_wrong_port_traps`.
#[test]
fn binding_the_wrong_port_traps() {
    let dir = scratch("net-wrong-bind-port");
    let source = dir.join("wrong_bind_port.ls");
    std::fs::write(
        &source,
        "edition 2;\n\
         fn main(world: World) -> [] int {\n\
             let Split { io, ffi, fs, heap, args, net } = split(world);\n\
             release(args); release(heap); release(ffi); release(fs); release(io);\n\
             let bound = narrow(net, \"1\");\n\
             var fd = 0;\n\
             borrow bound as &n in {\n\
                 fd = bind(n, 2);\n\
             }\n\
             release(bound);\n\
             return fd;\n\
         }\n",
    )
    .expect("a writable fixture");

    let exe = dir.join("wrong_bind_port");
    let build = Command::new(BIN)
        .args(["build".as_ref(), source.as_os_str(), "-o".as_ref(), exe.as_os_str()])
        .output()
        .expect("the compiler runs");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&exe).output().expect("the compiled program runs");
    assert!(!run.status.success(), "a port outside the bound should not succeed");
    assert_eq!(run.status.code(), None, "the process should be killed by a signal, not exit");

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------
// `examples/agent_guest/` and `examples/agent_supervisor/` -- the
// guest/supervisor exchange over plain HTTP (`docs/net.md` §5's #125
// recount)
// ---------------------------------------------------------------------

/// A lex-sys guest, POSTing to a lex-sys supervisor, and decoding the
/// `AgentViewMsg` it gets back.
///
/// The same shape as `a_lex_sys_client_fetches_from_a_lex_sys_server`:
/// `agent_supervisor` binds and accepts, `agent_guest` connects, and
/// neither is a Rust stand-in for the other half. `agent_guest` exits 3
/// when nothing is listening yet, so it is retried the same way `fetch`
/// is.
#[test]
fn a_lex_sys_guest_exchanges_a_view_with_a_lex_sys_supervisor() {
    use std::time::{Duration, Instant};
    let (supervisor_dir, supervisor) = build_example(
        "agent-supervisor",
        "examples/agent_supervisor/agent_supervisor.ls",
        "agent_supervisor",
    );
    let (guest_dir, guest) =
        build_example("agent-guest", "examples/agent_guest/agent_guest.ls", "agent_guest");

    let port = free_port().to_string();
    let mut child = Command::new(&supervisor)
        .args([port.as_str(), "write the report", "3"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the supervisor runs");
    let deadline = Instant::now() + Duration::from_secs(10);
    let run = loop {
        let run = Command::new(&guest)
            .args(["127.0.0.1", port.as_str()])
            .output()
            .expect("the guest runs");
        if run.status.code() != Some(3) || Instant::now() > deadline {
            break run;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = child.wait();
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "goal: write the report\nstep: 3\n",
        "`agent_guest` should print exactly the goal and step it decoded; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(0), "`agent_guest` should exit 0 on a decoded view");

    let _ = std::fs::remove_dir_all(&supervisor_dir);
    let _ = std::fs::remove_dir_all(&guest_dir);
}

/// A goal with characters that matter to JSON (`"` and `\`) survives the
/// round trip escaped rather than corrupting the message -- the same
/// property `examples/vsock/vsock.ls`'s own escaper has, checked here
/// end to end rather than by inspection.
#[test]
fn a_goal_needing_json_escaping_survives_the_round_trip() {
    use std::time::{Duration, Instant};
    let (supervisor_dir, supervisor) = build_example(
        "agent-supervisor-escape",
        "examples/agent_supervisor/agent_supervisor.ls",
        "agent_supervisor",
    );
    let (guest_dir, guest) =
        build_example("agent-guest-escape", "examples/agent_guest/agent_guest.ls", "agent_guest");

    let port = free_port().to_string();
    let mut child = Command::new(&supervisor)
        .args([port.as_str(), "say \"hi\" and go", "42"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the supervisor runs");
    let deadline = Instant::now() + Duration::from_secs(10);
    let run = loop {
        let run = Command::new(&guest)
            .args(["127.0.0.1", port.as_str()])
            .output()
            .expect("the guest runs");
        if run.status.code() != Some(3) || Instant::now() > deadline {
            break run;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = child.wait();
    // The decoder does not unescape (`examples/vsock/vsock.ls`'s own
    // documented limit): what the guest prints is the escaped form.
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "goal: say \\\"hi\\\" and go\nstep: 42\n",
        "the escaped goal should survive the round trip; stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(0), "`agent_guest` should exit 0 on a decoded view");

    let _ = std::fs::remove_dir_all(&supervisor_dir);
    let _ = std::fs::remove_dir_all(&guest_dir);
}
