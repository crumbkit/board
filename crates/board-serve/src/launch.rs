//! `board ui open`: reuse the board's running server or start one, then
//! open a page of it in a browser.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use board_model::{Code, LogicalPath};
use board_ops::{Board, PageError};

use crate::{Token, read_record};

/// How long `open` waits for a server it started to answer.
const START_WAIT: Duration = Duration::from_secs(5);

/// How long a request to a recorded server may take.
const ASK_WAIT: Duration = Duration::from_secs(1);

/// What `open` needs beyond the board: where the cache is, how to start a
/// server, and how to open a browser.
#[derive(Clone, Debug)]
pub struct Launch {
    pub cache: PathBuf,
    /// The `board` binary that runs `board serve`.
    pub binary: PathBuf,
    /// The program that opens a URL, given the URL as its one argument. When
    /// there is none, `open` on macOS and `xdg-open` elsewhere.
    pub browser: Option<OsString>,
}

/// The page `open` opened, and whether it started the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub binding: LogicalPath,
    pub port: u16,
    pub started: bool,
}

/// Why `open` opened nothing.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Page(#[from] PageError),
    #[error("{reason}")]
    Failed { reason: String },
    /// The server refused a request, with its code, message and hint.
    #[error("the server refused {request} with {status}: {message}")]
    Refused {
        request: String,
        status: u16,
        code: Code,
        message: String,
        hint: String,
    },
}

impl OpenError {
    pub fn code(&self) -> Code {
        match self {
            Self::Page(error) => error.code(),
            Self::Failed { .. } => Code::UiOpenFailed,
            Self::Refused { code, .. } => *code,
        }
    }
}

fn failed(reason: String) -> OpenError {
    OpenError::Failed { reason }
}

/// Opens the page of the binding at `path`: reuses the board's server when
/// the one its server file names proves it holds the recorded token, else
/// starts `board serve` detached and waits for it. It then asks the server,
/// with the token, for a launch code, which also keeps the server awake
/// until the browser arrives. The server writes an opener page in the
/// cache, readable only by this user, that sends the browser to the page
/// with the code in the URL's fragment, and the browser opens it.
pub fn open(board: &Board, launch: &Launch, path: &LogicalPath) -> Result<Opened, OpenError> {
    board.binding(path)?;
    let root = board.repository().path();
    let (port, token, started) = if let Some((port, token)) = live(&launch.cache, root)? {
        (port, token, false)
    } else {
        let arguments = [OsStr::new("-C"), root.as_os_str(), OsStr::new("serve")];
        spawn(launch.binary.as_os_str(), &arguments)
            .map_err(|error| failed(format!("board serve could not be started: {error}")))?;
        let (port, token) = wait(&launch.cache, root)?.ok_or_else(|| {
            failed(format!(
                "board serve did not answer within {}s",
                START_WAIT.as_secs()
            ))
        })?;
        (port, token, true)
    };
    let opener = ask(port, "POST /api/codes", path.as_str(), Some(&token))?;
    let url = file_url(Path::new(&opener));
    let program = match &launch.browser {
        Some(program) => program.as_os_str(),
        None if cfg!(target_os = "macos") => OsStr::new("open"),
        None => OsStr::new("xdg-open"),
    };
    spawn(program, &[OsStr::new(&url)])
        .map_err(|error| failed(format!("{} could not be run: {error}", program.display())))?;
    Ok(Opened {
        binding: path.clone(),
        port,
        started,
    })
}

/// The `file://` URL of `path`, with every byte but letters, digits and
/// `-._~/` percent-encoded.
pub(crate) fn file_url(path: &Path) -> String {
    path.as_os_str()
        .as_bytes()
        .iter()
        .fold(String::from("file://"), |mut url, &byte| {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                url.push(char::from(byte));
            } else {
                // Writing to a String cannot fail.
                let _ = write!(url, "%{byte:02X}");
            }
            url
        })
}

/// The port and token of the server the server file names, when it proves
/// it holds the token. A server file this user could not have written is
/// refused.
fn live(cache: &Path, root: &Path) -> Result<Option<(u16, Token)>, OpenError> {
    let bytes = board_index::read_server(cache, root)
        .map_err(|error| failed(format!("the server file cannot be trusted: {error}")))?;
    let Some((port, token)) = bytes.as_deref().and_then(read_record) else {
        return Ok(None);
    };
    Ok(proves(port, &token).then_some((port, token)))
}

/// Whether the server at `port` answers a probe with the proof that it
/// holds `token`, for a fresh nonce.
fn proves(port: u16, token: &Token) -> bool {
    let Ok(nonce) = Token::new() else {
        return false;
    };
    let nonce = nonce.expose();
    ask(port, &format!("GET /api/probe?nonce={nonce}"), "", None).is_ok_and(|body| {
        constant_time_eq::constant_time_eq(body.as_bytes(), token.prove(nonce).as_bytes())
    })
}

/// The body of the server at `port`'s 200 reply to the request whose method
/// and URL are `line` and whose body is `body`, sent with `token` as a
/// bearer token when there is one. Any other reply is refused with the
/// server's code, message and hint.
pub(crate) fn ask(
    port: u16,
    line: &str,
    body: &str,
    token: Option<&Token>,
) -> Result<String, OpenError> {
    let unanswered = |error: std::io::Error| failed(format!("{line} got no answer: {error}"));
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, ASK_WAIT).map_err(unanswered)?;
    let bearer = token.map_or(String::new(), |token| {
        format!("Authorization: Bearer {}\r\n", token.expose())
    });
    let request = format!(
        "{line} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{bearer}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut response = Vec::new();
    stream
        .set_read_timeout(Some(ASK_WAIT))
        .and_then(|()| stream.write_all(request.as_bytes()))
        .and_then(|()| stream.take(1 << 20).read_to_end(&mut response))
        .map_err(unanswered)?;
    let response = String::from_utf8_lossy(&response);
    let malformed = || failed(format!("{line} got a reply that is not HTTP"));
    let (head, body) = response.split_once("\r\n\r\n").ok_or_else(malformed)?;
    let status = head
        .strip_prefix("HTTP/1.1 ")
        .and_then(|rest| rest.get(..3))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    if status == 200 {
        return Ok(body.to_owned());
    }
    let refusal: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let field = |name: &str| refusal[name].as_str().unwrap_or_default().to_owned();
    Err(OpenError::Refused {
        request: line.to_owned(),
        status,
        code: Code::parse(&field("code")).unwrap_or(Code::UiOpenFailed),
        message: field("message"),
        hint: field("hint"),
    })
}

/// Waits for a server that was just started to answer.
fn wait(cache: &Path, root: &Path) -> Result<Option<(u16, Token)>, OpenError> {
    let start = Instant::now();
    while start.elapsed() < START_WAIT {
        if let Some(found) = live(cache, root)? {
            return Ok(Some(found));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(None)
}

/// Starts `program` in its own process group with no terminal streams, and
/// does not wait for it: the server and the browser outlive `board ui open`.
#[expect(
    clippy::disallowed_methods,
    reason = "board-serve's launch module owns the server and browser children"
)]
fn spawn(program: &OsStr, arguments: &[&OsStr]) -> std::io::Result<()> {
    std::process::Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}
