//! The server's refusals, each paired with the request it admits, one
//! server run end to end over a socket, and `open` against a running
//! server.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use board_model::{Code, LogicalPath};
use board_ops::{Board, Page, PageError};
use board_scenario::{Item, Scenario};

use super::*;

const PORT: u16 = 4321;
const TOKEN: &str = "a1b2";
const HOST: &str = "127.0.0.1:4321";

fn request(url: &str, host: &str, authorization: Option<&str>) -> Request {
    Request {
        method: "GET".to_owned(),
        url: url.to_owned(),
        host: Some(host.to_owned()),
        authorization: authorization.map(str::to_owned),
        origin: None,
        fetch_site: None,
        body: String::new(),
    }
}

fn post(url: &str, body: &str, authorization: Option<&str>) -> Request {
    Request {
        method: "POST".to_owned(),
        body: body.to_owned(),
        ..request(url, HOST, authorization)
    }
}

/// Codes whose openers would go in a cache that no test here writes.
fn no_codes() -> Codes {
    Codes::new(Path::new("/nonexistent"), Path::new("/boards/a"))
}

fn page(path: &LogicalPath) -> Result<Page, PageError> {
    if path.as_str() == "/bindings/review" {
        Ok(Page {
            binding: path.clone(),
            view: "/views/review".parse().unwrap(),
            slots: Vec::new(),
        })
    } else {
        Err(PageError::NotABinding { path: path.clone() })
    }
}

fn reply(request: &Request) -> Reply {
    let token = Token::from(TOKEN);
    respond(request, PORT, &token, &mut no_codes(), Instant::now(), page)
}

/// The reply to a request that must be answered without reading the board.
fn refusal(request: &Request) -> Reply {
    let token = Token::from(TOKEN);
    respond(
        request,
        PORT,
        &token,
        &mut no_codes(),
        Instant::now(),
        |_| panic!("the request read the board"),
    )
}

const BEARER: Option<&str> = Some("Bearer a1b2");

#[test]
fn a_host_that_is_not_this_server_is_refused_and_this_servers_host_is_admitted() {
    for host in [
        "localhost:4321",
        "127.0.0.1:4322",
        "evil.example:4321",
        "127.0.0.1",
    ] {
        let refused = refusal(&request("/api/bindings/review", host, BEARER));
        assert_eq!(
            (refused.status, refused.code),
            (403, Some(Code::ServeHost)),
            "{host}"
        );
    }
    let mut missing = request("/", HOST, None);
    missing.host = None;
    assert_eq!(reply(&missing).code, Some(Code::ServeHost));
    let admitted = reply(&request("/api/bindings/review", HOST, BEARER));
    assert_eq!((admitted.status, admitted.code), (200, None));
}

#[test]
fn an_api_request_without_the_launchs_bearer_token_is_refused_and_one_with_it_is_admitted() {
    for authorization in [
        None,
        Some("Bearer "),
        Some("Bearer a1b3"),
        Some("Bearer a1b2c"),
        Some("a1b2"),
        Some("Basic a1b2"),
    ] {
        let refused = refusal(&request("/api/bindings/review", HOST, authorization));
        assert_eq!(
            (refused.status, refused.code, refused.authorised),
            (401, Some(Code::ServeToken), false),
            "{authorization:?}"
        );
    }
    // A page opened without a launch code sends an empty bearer token.
    let empty = refusal(&request("/api/bindings/review", HOST, Some("Bearer ")));
    let body: Value = serde_json::from_slice(&empty.body).unwrap();
    assert_eq!(body["message"], "the request carries no token");
    assert!(body["hint"].as_str().unwrap().contains("board ui open"));
    let admitted = reply(&request("/api/bindings/review", HOST, BEARER));
    assert_eq!((admitted.status, admitted.authorised), (200, true));
}

#[test]
fn the_app_is_served_without_a_token_and_does_not_keep_the_server_awake() {
    for (route, media_type) in [
        ("/", "text/html; charset=utf-8"),
        ("/assets/app.js", "text/javascript"),
        ("/assets/app.css", "text/css"),
    ] {
        let served = reply(&request(route, HOST, None));
        assert_eq!(
            (served.status, served.content_type, served.authorised),
            (200, media_type, false),
            "{route}"
        );
    }
    assert!(
        reply(&request("/", HOST, None))
            .body
            .starts_with(b"<!doctype html>")
    );
}

#[test]
fn a_route_the_server_does_not_answer_is_refused_and_the_app_is_served() {
    for (method, url) in [
        ("GET", "/etc/passwd"),
        ("GET", "/index.html"),
        ("POST", "/"),
        ("GET", "/api/session"),
        ("GET", "/api/codes"),
        ("GET", "/api/gate/x"),
    ] {
        let mut unknown = request(url, HOST, BEARER);
        unknown.method = method.to_owned();
        let refused = reply(&unknown);
        assert_eq!(
            (refused.status, refused.code),
            (404, Some(Code::ServeUnknownRoute)),
            "{method} {url}"
        );
    }
    assert_eq!(reply(&request("/", HOST, None)).status, 200);
}

#[test]
fn a_page_for_a_record_that_is_not_a_binding_is_refused_and_a_bindings_page_is_served() {
    let refused = reply(&request("/api/bindings/other", HOST, BEARER));
    assert_eq!(
        (refused.status, refused.code),
        (422, Some(Code::UiNotABinding))
    );
    let body: Value = serde_json::from_slice(&refused.body).unwrap();
    assert_eq!(body["code"], "board::ui::not_a_binding");
    assert!(
        body["hint"]
            .as_str()
            .unwrap()
            .contains("board ls /bindings")
    );

    let served = reply(&request("/api/bindings/review", HOST, BEARER));
    let body: Value = serde_json::from_slice(&served.body).unwrap();
    assert_eq!(body["view"], "/views/review");
}

#[test]
fn a_probe_without_a_hex_nonce_is_refused_and_one_with_a_nonce_gets_the_tokens_proof() {
    for url in [
        "/api/probe",
        "/api/probe?nonce=",
        "/api/probe?nonce=0123",
        "/api/probe?nonce=zzzzzzzzzzzzzzzzzzzz",
    ] {
        let refused = refusal(&request(url, HOST, None));
        assert_eq!(refused.code, Some(Code::ServeUnknownRoute), "{url}");
    }
    let nonce = "0123456789abcdef";
    let proved = refusal(&request(&format!("/api/probe?nonce={nonce}"), HOST, None));
    assert_eq!((proved.status, proved.authorised), (200, false));
    assert_eq!(&*proved.body, Token::from(TOKEN).prove(nonce).as_bytes());
    assert_ne!(
        Token::from("a1b3").prove(nonce),
        Token::from(TOKEN).prove(nonce)
    );
}

#[test]
fn a_token_is_sixty_four_hex_digits_made_fresh_each_launch_and_never_printed() {
    let (one, two) = (Token::new().unwrap(), Token::new().unwrap());
    assert_eq!(one.expose().len(), 64);
    assert!(one.expose().bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(!one.matches(two.expose()));
    assert!(one.matches(one.expose()));
    assert_eq!(format!("{one:?}"), "Token([redacted])");
    let printed = format!("{:?}", post("/", "c0de", BEARER));
    assert!(
        !printed.contains("a1b2") && !printed.contains("c0de"),
        "{printed}"
    );
    assert!(printed.contains("[redacted]"), "{printed}");
}

#[test]
fn the_opener_sends_the_browser_to_the_page_with_the_launch_code_in_the_fragment() {
    let opener = codes::opener(PORT, "c0de", &"/bindings/b".parse().unwrap());
    let text = String::from_utf8(opener).unwrap();
    assert!(
        text.contains("content=\"0;url=http://127.0.0.1:4321/#code=c0de&amp;/bindings/b\""),
        "{text}"
    );
}

/// Makes a launch code for `/bindings/review` in `codes` at `now`, and
/// returns the opener's path and the code.
fn launch_code(codes: &mut Codes, now: Instant) -> (std::path::PathBuf, String) {
    let token = Token::from(TOKEN);
    let made = respond(
        &post("/api/codes", "/bindings/review", BEARER),
        PORT,
        &token,
        codes,
        now,
        |_| panic!("making a code read the board"),
    );
    assert_eq!((made.status, made.authorised), (200, true));
    let opener = std::path::PathBuf::from(String::from_utf8(made.body.into_owned()).unwrap());
    let text = std::fs::read_to_string(&opener).unwrap();
    let code = text
        .split("#code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    (opener, code.to_owned())
}

fn exchange(codes: &mut Codes, code: &str, now: Instant) -> Reply {
    let token = Token::from(TOKEN);
    respond(
        &post("/api/session", code, None),
        PORT,
        &token,
        codes,
        now,
        |_| panic!("an exchange read the board"),
    )
}

#[test]
fn a_launch_code_is_exchanged_once_for_the_token_and_a_second_exchange_is_refused_as_used() {
    let cache = tempfile::tempdir().unwrap();
    let mut codes = Codes::new(cache.path(), Path::new("/boards/a"));
    let now = Instant::now();
    let (opener, code) = launch_code(&mut codes, now);
    assert_eq!(code.len(), 64);
    assert_eq!(
        std::fs::metadata(&opener).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let admitted = exchange(&mut codes, &code, now);
    assert_eq!((admitted.status, admitted.authorised), (200, true));
    assert_eq!(&*admitted.body, TOKEN.as_bytes());
    assert!(!opener.exists(), "the opener outlived its code");
    let refused = exchange(&mut codes, &code, now);
    assert_eq!(
        (refused.status, refused.code),
        (401, Some(Code::ServeCodeUsed))
    );
}

#[test]
fn an_unknown_launch_code_is_refused_and_a_code_this_server_made_is_admitted() {
    let cache = tempfile::tempdir().unwrap();
    let mut codes = Codes::new(cache.path(), Path::new("/boards/a"));
    let now = Instant::now();
    let (_, code) = launch_code(&mut codes, now);
    // The code with its last digit changed, never to the digit it has.
    let last = if code.ends_with('0') { '1' } else { '0' };
    let wrong = format!("{}{last}", &code[..63]);
    assert_ne!(wrong, code);
    for unknown in [wrong.as_str(), "", TOKEN] {
        let refused = exchange(&mut codes, unknown, now);
        assert_eq!(
            (refused.status, refused.code),
            (401, Some(Code::ServeCodeUnknown)),
            "{unknown}"
        );
    }
    assert_eq!(exchange(&mut codes, &code, now).status, 200);
}

#[test]
fn a_launch_code_past_its_minute_is_refused_as_expired_and_its_opener_removed_and_one_within_it_is_admitted()
 {
    let cache = tempfile::tempdir().unwrap();
    let mut codes = Codes::new(cache.path(), Path::new("/boards/a"));
    let now = Instant::now();
    let (late_opener, late) = launch_code(&mut codes, now);
    let (_, timely) = launch_code(&mut codes, now);
    let after = now + codes::CODE_LIFE + Duration::from_secs(1);

    let refused = exchange(&mut codes, &late, after);
    assert_eq!(
        (refused.status, refused.code),
        (401, Some(Code::ServeCodeExpired))
    );
    codes.expire(after);
    assert!(!late_opener.exists(), "the opener outlived its code");
    let admitted = exchange(&mut codes, &timely, now + codes::CODE_LIFE);
    assert_eq!(admitted.status, 200);
}

#[test]
fn a_launch_code_exchange_from_another_site_is_refused_and_one_from_this_servers_page_is_admitted()
{
    let cache = tempfile::tempdir().unwrap();
    let mut codes = Codes::new(cache.path(), Path::new("/boards/a"));
    let now = Instant::now();
    let (_, code) = launch_code(&mut codes, now);
    let token = Token::from(TOKEN);
    let mut send = |origin: Option<&str>, site: Option<&str>| {
        let request = Request {
            origin: origin.map(str::to_owned),
            fetch_site: site.map(str::to_owned),
            ..post("/api/session", &code, None)
        };
        respond(&request, PORT, &token, &mut codes, now, |_| {
            panic!("an exchange read the board")
        })
    };
    for (origin, site) in [
        (Some("http://evil.example"), None),
        (Some("http://127.0.0.1:4322"), Some("same-site")),
        (Some("null"), None),
        (None, Some("cross-site")),
    ] {
        let refused = send(origin, site);
        assert_eq!(
            (refused.status, refused.code),
            (403, Some(Code::ServeOrigin)),
            "{origin:?} {site:?}"
        );
    }
    let admitted = send(Some("http://127.0.0.1:4321"), Some("same-origin"));
    assert_eq!((admitted.status, admitted.code), (200, None));
}

#[test]
fn making_a_launch_code_without_the_token_or_for_a_path_outside_bindings_is_refused_and_with_both_admitted()
 {
    let cache = tempfile::tempdir().unwrap();
    let mut codes = Codes::new(cache.path(), Path::new("/boards/a"));
    let token = Token::from(TOKEN);
    let mut make = |body: &str, authorization| {
        respond(
            &post("/api/codes", body, authorization),
            PORT,
            &token,
            &mut codes,
            Instant::now(),
            |_| panic!("making a code read the board"),
        )
    };
    let refused = make("/bindings/review", Some("Bearer a1b3"));
    assert_eq!(
        (refused.status, refused.code),
        (401, Some(Code::ServeToken))
    );
    let refused = make("/views/review", BEARER);
    assert_eq!(
        (refused.status, refused.code),
        (422, Some(Code::UiNotABinding))
    );
    assert_eq!(make("/bindings/review", BEARER).status, 200);
}

#[test]
fn the_openers_file_url_percent_encodes_every_byte_a_url_path_cannot_hold() {
    let path = std::path::Path::new("/Users/a b/#x?/ü%/open-1.html");
    assert_eq!(
        launch::file_url(path),
        "file:///Users/a%20b/%23x%3F/%C3%BC%25/open-1.html"
    );
}

#[test]
fn the_embedded_app_is_every_file_the_build_writes() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    let mut found = Vec::new();
    let mut directories = vec![dist.clone()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else {
                let relative = path.strip_prefix(&dist).unwrap().to_str().unwrap();
                found.push(format!("/{}", relative.replace("index.html", "")));
            }
        }
    }
    found.sort();
    let mut embedded: Vec<String> = respond::APP
        .iter()
        .map(|(route, ..)| (*route).to_owned())
        .collect();
    embedded.sort();
    assert_eq!(found, embedded);
}

fn get(port: u16, host: &str, url: &str, authorization: Option<&str>) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let authorization =
        authorization.map_or(String::new(), |value| format!("Authorization: {value}\r\n"));
    write!(
        stream,
        "GET {url} HTTP/1.1\r\nHost: {host}\r\n{authorization}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn board_scenario() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            "views/v.json",
            Item::File(r#"{"$type": "ui/view/v1", "slots": []}"#),
        )
        .write(
            "bindings/b.json",
            Item::File(r#"{"$type": "ui/binding/v1", "view": "/views/v", "slots": {}}"#),
        );
    scenario
}

/// Runs a server for the board at `workdir` while `during` runs with its
/// port and token, then stops it.
fn with_server<T>(
    workdir: &std::path::Path,
    cache: &std::path::Path,
    during: impl FnOnce(u16, Token) -> T,
) -> (T, Served) {
    let root = Board::open(workdir, None)
        .unwrap()
        .repository()
        .path()
        .to_owned();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            let board = Board::open(workdir, None).unwrap();
            serve(&board, cache, Duration::from_mins(1), &stop)
        });
        let (port, token) = loop {
            if let Some(bytes) = board_index::read_server(cache, &root).unwrap() {
                break read_record(&bytes).unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let result = during(port, token);
        stop.store(true, Ordering::Relaxed);
        (result, server.join().unwrap().unwrap())
    })
}

#[test]
fn a_server_answers_over_a_socket_and_removes_its_file_when_stopped() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let workdir = scenario.workdir().to_owned();
    let ((), served) = with_server(&workdir, cache.path(), |port, token| {
        let host = format!("127.0.0.1:{port}");
        let refused = get(port, &format!("localhost:{port}"), "/", None);
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        let app = get(port, &host, "/", None);
        assert!(app.starts_with("HTTP/1.1 200"), "{app}");
        assert!(app.contains("Content-Security-Policy: default-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; object-src 'none'; frame-ancestors 'none'"));
        assert!(app.contains("Cross-Origin-Resource-Policy: same-origin"));
        assert!(app.contains("Cross-Origin-Opener-Policy: same-origin"));
        let bearer = format!("Bearer {}", token.expose());
        let page = get(port, &host, "/api/bindings/b", Some(&bearer));
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    });
    assert_eq!(served.requests, 3);
    let root = Board::open(&workdir, None)
        .unwrap()
        .repository()
        .path()
        .to_owned();
    assert_eq!(board_index::read_server(cache.path(), &root).unwrap(), None);
}

#[test]
fn a_server_with_no_authorised_request_exits_when_idle() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let board = Board::open(scenario.workdir(), None).unwrap();
    let start = Instant::now();
    let served = serve(
        &board,
        cache.path(),
        Duration::from_millis(50),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(served.requests, 0);
    assert!(start.elapsed() < Duration::from_secs(30));
}

/// A launch whose browser is a script that records the URL it is asked to
/// open, and whose `board` binary cannot start.
fn launch(scenario: &Scenario, cache: &std::path::Path) -> (Launch, std::path::PathBuf) {
    let opened = cache.join("opened");
    let script = format!("#!/bin/sh\nprintf %s \"$1\" > {}\n", opened.display());
    scenario.write("browser.sh", Item::Executable(&script));
    let launch = Launch {
        cache: cache.to_owned(),
        binary: "/nonexistent/board".into(),
        browser: Some(scenario.workdir().join("browser.sh").into()),
    };
    (launch, opened)
}

/// What the browser was asked to open, waiting for the detached script.
fn opened(file: &std::path::Path) -> String {
    let deadline = Instant::now() + Duration::from_mins(1);
    while Instant::now() < deadline {
        if let Ok(url) = std::fs::read_to_string(file) {
            return url;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the browser was never run");
}

#[test]
fn opening_a_page_with_no_server_running_is_refused_and_opening_it_with_one_running_is_admitted() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let (launch, opened_file) = launch(&scenario, cache.path());
    let workdir = scenario.workdir().to_owned();
    let board = Board::open(&workdir, None).unwrap();
    let path: LogicalPath = "/bindings/b".parse().unwrap();

    let refused = open(&board, &launch, &path).unwrap_err();
    assert_eq!(refused.code(), Code::UiOpenFailed);

    // The opener is read while the server runs, since it removes the opener
    // when it stops.
    let ((opened_page, url, opener_text, mode), served) =
        with_server(&workdir, cache.path(), |_, _| {
            let opened_page = open(&board, &launch, &path).unwrap();
            let url = opened(&opened_file);
            let opener = url.strip_prefix("file://").unwrap().to_owned();
            let text = std::fs::read_to_string(&opener).unwrap();
            let mode = std::fs::metadata(&opener).unwrap().permissions().mode();
            (opened_page, url, text, mode)
        });
    assert_eq!((&opened_page.binding, opened_page.started), (&path, false));
    // The probe, then the authorised request for a launch code, which keeps
    // the server awake.
    assert_eq!(served.requests, 2);
    let opener = std::path::PathBuf::from(url.strip_prefix("file://").unwrap());
    assert_eq!(url, launch::file_url(&opener));
    assert!(
        opener
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("open-")
    );
    assert_eq!(mode & 0o777, 0o600);
    let text = opener_text;
    let url = format!("http://127.0.0.1:{}/#code=", opened_page.port);
    assert!(text.contains(&url), "{text}");
}

#[test]
fn a_server_file_whose_server_cannot_prove_its_token_is_not_reused_and_a_true_one_is() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let (launch, _) = launch(&scenario, cache.path());
    let workdir = scenario.workdir().to_owned();
    let board = Board::open(&workdir, None).unwrap();
    let root = board.repository().path().to_owned();
    let path: LogicalPath = "/bindings/b".parse().unwrap();

    let ((refused, admitted), _) = with_server(&workdir, cache.path(), |port, token| {
        let forged = server_record(port, &Token::from("a1b2"));
        board_index::write_server(cache.path(), &root, &forged).unwrap();
        let refused = open(&board, &launch, &path).map(|opened| opened.started);
        let true_record = server_record(port, &token);
        board_index::write_server(cache.path(), &root, &true_record).unwrap();
        let admitted = open(&board, &launch, &path).map(|opened| opened.started);
        (refused, admitted)
    });
    assert_eq!(refused.unwrap_err().code(), Code::UiOpenFailed);
    assert!(!admitted.unwrap());
}

#[test]
fn opening_a_binding_outside_bindings_is_refused_and_one_under_bindings_is_admitted() {
    let scenario = board_scenario();
    scenario.write(
        "pages/b.json",
        Item::File(r#"{"$type": "ui/binding/v1", "view": "/views/v", "slots": {}}"#),
    );
    let cache = tempfile::tempdir().unwrap();
    let (launch, _) = launch(&scenario, cache.path());
    let workdir = scenario.workdir().to_owned();
    let board = Board::open(&workdir, None).unwrap();
    let ((refused, admitted), _) = with_server(&workdir, cache.path(), |_, _| {
        (
            open(&board, &launch, &"/pages/b".parse().unwrap()),
            open(&board, &launch, &"/bindings/b".parse().unwrap()),
        )
    });
    assert_eq!(refused.unwrap_err().code(), Code::UiNotABinding);
    assert!(admitted.is_ok());
}

#[test]
fn a_refused_request_from_open_carries_the_servers_code_and_hint_and_an_admitted_one_its_body() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let workdir = scenario.workdir().to_owned();
    let ((refused, admitted), _) = with_server(&workdir, cache.path(), |port, token| {
        let wrong = Token::from("a1b2");
        (
            launch::ask(port, "POST /api/codes", "/bindings/b", Some(&wrong)),
            launch::ask(port, "POST /api/codes", "/bindings/b", Some(&token)),
        )
    });
    let OpenError::Refused {
        status, code, hint, ..
    } = refused.unwrap_err()
    else {
        panic!("the refusal lost the server's reply");
    };
    assert_eq!((status, code), (401, Code::ServeToken));
    assert!(hint.contains("board ui open"), "{hint}");
    assert!(admitted.unwrap().contains("open-"));
}

#[test]
fn a_stopped_server_removes_the_opener_of_an_unused_code_and_keeps_none_after_a_used_one() {
    let scenario = board_scenario();
    let cache = tempfile::tempdir().unwrap();
    let workdir = scenario.workdir().to_owned();
    let (openers, _) = with_server(&workdir, cache.path(), |port, token| {
        let opener = |token: &Token| {
            launch::ask(port, "POST /api/codes", "/bindings/b", Some(token)).unwrap()
        };
        let used = opener(&token);
        let text = std::fs::read_to_string(&used).unwrap();
        let code = text
            .split("#code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        launch::ask(port, "POST /api/session", code, None).unwrap();
        let unused = opener(&token);
        assert!(Path::new(&unused).exists());
        (used, unused)
    });
    assert!(
        !Path::new(&openers.0).exists(),
        "a used code's opener was kept"
    );
    assert!(
        !Path::new(&openers.1).exists(),
        "an unused code's opener outlived the server"
    );
}
