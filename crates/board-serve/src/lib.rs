//! `board serve`: the local HTTP server that draws a board's pages, and
//! `board ui open`, which starts or reuses it and opens a page
//! (docs/architecture/ui/serve.md). Only this crate depends on `tiny_http`.

use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use board_model::Code;
use board_ops::Board;
use secrecy::{ExposeSecret as _, SecretString};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

mod codes;
mod launch;
mod respond;

pub use crate::codes::Codes;

pub use crate::launch::{Launch, OpenError, Opened, open};
pub use crate::respond::{Reply, Request, respond};

/// How long `board serve` waits for an authorised request before it exits.
pub const DEFAULT_IDLE: Duration = Duration::from_mins(30);

/// How often the server looks at its stop flag while it waits.
const TICK: Duration = Duration::from_millis(200);

/// The secret a page must present on every API request: 32 random bytes in
/// hex, made once per launch. `Debug` never prints it.
#[derive(Clone)]
pub struct Token(SecretString);

impl Token {
    pub fn new() -> Result<Self, ServeError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| ServeError::Random {
            reason: error.to_string(),
        })?;
        Ok(Self(hex(&bytes).into()))
    }

    /// Whether `presented` is this token, compared in constant time.
    pub fn matches(&self, presented: &str) -> bool {
        constant_time_eq::constant_time_eq(self.expose().as_bytes(), presented.as_bytes())
    }

    /// The proof that a server holds this token, for the `nonce` a client
    /// sent: SHA-256 of the token followed by the nonce, in hex. The client
    /// never sends the token itself.
    pub fn prove(&self, nonce: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(self.expose().as_bytes());
        digest.update(nonce.as_bytes());
        hex(&digest.finalize())
    }

    pub(crate) fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl From<&str> for Token {
    fn from(text: &str) -> Self {
        Self(text.into())
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token([redacted])")
    }
}

/// `bytes` in lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// What `board serve` reports when it exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Served {
    pub port: u16,
    pub requests: u64,
    pub idle: Duration,
}

/// Why `board serve` could not start or keep serving.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("no random token could be made: {reason}")]
    Random { reason: String },
    #[error("127.0.0.1 accepts no listener: {reason}")]
    Bind { reason: String },
    #[error("the server file {} cannot be written: {source}", file.display())]
    File {
        file: PathBuf,
        source: std::io::Error,
    },
    #[error("the server stopped: {source}")]
    Accept { source: std::io::Error },
}

impl ServeError {
    pub fn code(&self) -> Code {
        Code::ServeFailed
    }
}

/// The contents of the server file: the port and the token.
pub(crate) fn server_record(port: u16, token: &Token) -> Vec<u8> {
    json!({"port": port, "token": token.expose()})
        .to_string()
        .into_bytes()
}

/// A server file's port and token, when it reads as one.
pub(crate) fn read_record(bytes: &[u8]) -> Option<(u16, Token)> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let port = u16::try_from(value.get("port")?.as_u64()?).ok()?;
    let token = value.get("token")?.as_str()?;
    Some((port, Token::from(token)))
}

/// Serves `board`'s pages on 127.0.0.1 at a port the system picks, until no
/// authorised request arrives for `idle` or `stop` is set. The port and
/// token go in the server file in the board's cache under `cache`,
/// which is removed on exit while it still names this server.
pub fn serve(
    board: &Board,
    cache: &Path,
    idle: Duration,
    stop: &AtomicBool,
) -> Result<Served, ServeError> {
    let token = Token::new()?;
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|error| ServeError::Bind {
        reason: error.to_string(),
    })?;
    let port = server
        .server_addr()
        .to_ip()
        .map_or(0, |address| address.port());
    let root = board.repository().path();
    let record = server_record(port, &token);
    board_index::write_server(cache, root, &record).map_err(|source| ServeError::File {
        file: board_index::server_file(cache, root),
        source,
    })?;
    let mut codes = Codes::new(cache, root);
    let outcome = run(board, &server, port, &token, &mut codes, idle, stop);
    codes.close();
    // The file only helps `board ui open` find a live server; one left
    // behind fails the probe, which `open` treats as no server.
    let _ = board_index::remove_server(cache, root, &record);
    outcome
}

/// Answers requests until no authorised one arrives for `idle`, or `stop`
/// is set.
fn run(
    board: &Board,
    server: &tiny_http::Server,
    port: u16,
    token: &Token,
    codes: &mut Codes,
    idle: Duration,
    stop: &AtomicBool,
) -> Result<Served, ServeError> {
    let mut requests = 0;
    let mut last = Instant::now();
    loop {
        codes.expire(Instant::now());
        let remaining = idle.checked_sub(last.elapsed());
        let Some(remaining) = remaining.filter(|_| !stop.load(Ordering::Relaxed)) else {
            return Ok(Served {
                port,
                requests,
                idle,
            });
        };
        let Some(mut request) = server
            .recv_timeout(remaining.min(TICK))
            .map_err(|source| ServeError::Accept { source })?
        else {
            continue;
        };
        requests += 1;
        let reply = respond(
            &Request::of(&mut request),
            port,
            token,
            codes,
            Instant::now(),
            |path| board.page(path),
        );
        if reply.authorised {
            last = Instant::now();
        }
        // A client that went away is not the server's failure.
        let _ = request.respond(reply.into_response());
    }
}

#[cfg(test)]
mod tests;
