//! How the server answers one request. Every request must name the server
//! by `127.0.0.1:<port>` in its Host header, which refuses pages that reach
//! it through DNS rebinding. The app's files and the launch code exchange
//! are then open to anyone; every other API request must also carry the
//! launch's token as a bearer token.

use std::borrow::Cow;
use std::fmt;
use std::time::Instant;

use board_model::{Code, Fields, LogicalPath};
use board_ops::{Page, PageError};
use serde_json::json;

use crate::{Codes, Token};

/// The most of a request's body the server reads.
const BODY_LIMIT: u64 = 4096;

/// The built app, `ui/dist`, embedded in the binary: each route, its media
/// type and its bytes.
pub(crate) const APP: [(&str, &str, &[u8]); 3] = [
    (
        "/",
        "text/html; charset=utf-8",
        include_bytes!("../../../ui/dist/index.html"),
    ),
    (
        "/assets/app.js",
        "text/javascript",
        include_bytes!("../../../ui/dist/assets/app.js"),
    ),
    (
        "/assets/app.css",
        "text/css",
        include_bytes!("../../../ui/dist/assets/app.css"),
    ),
];

/// The headers every reply carries: nothing is cached, nothing is sniffed,
/// no referrer leaves the page, no other site may load a reply or share the
/// page's window, and the page loads only its own code.
const HEADERS: [(&str, &str); 6] = [
    ("Cache-Control", "no-store"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    (
        "Content-Security-Policy",
        "default-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; object-src 'none'; frame-ancestors 'none'",
    ),
];

/// The parts of a request the server reads. `Debug` never prints the
/// Authorization header or the body, which hold the token and launch codes.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub host: Option<String>,
    pub authorization: Option<String>,
    pub origin: Option<String>,
    pub fetch_site: Option<String>,
    pub body: String,
}

impl Request {
    pub(crate) fn of(request: &mut tiny_http::Request) -> Self {
        let mut body = String::new();
        // A body that cannot be read as text reads as empty, which every
        // route refuses.
        let _ = std::io::Read::read_to_string(
            &mut std::io::Read::take(request.as_reader(), BODY_LIMIT),
            &mut body,
        );
        let header = |name: &'static str| {
            request
                .headers()
                .iter()
                .find(|header| header.field.equiv(name))
                .map(|header| header.value.as_str().to_owned())
        };
        Self {
            method: request.method().as_str().to_owned(),
            url: request.url().to_owned(),
            host: header("Host"),
            authorization: header("Authorization"),
            origin: header("Origin"),
            fetch_site: header("Sec-Fetch-Site"),
            body,
        }
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("host", &self.host)
            .field(
                "authorization",
                &self.authorization.as_ref().map(|_| "[redacted]"),
            )
            .field("origin", &self.origin)
            .field("fetch_site", &self.fetch_site)
            .field("body", &"[redacted]")
            .finish()
    }
}

/// A reply: its status, its body, the code of a refusal, and whether the
/// request carried the token, which alone keeps the server from going idle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Cow<'static, [u8]>,
    pub code: Option<Code>,
    pub authorised: bool,
}

impl Reply {
    fn ok(content_type: &'static str, body: Cow<'static, [u8]>) -> Self {
        Self {
            status: 200,
            content_type,
            body,
            code: None,
            authorised: false,
        }
    }

    fn refused(status: u16, code: Code, message: &str, hint: &str) -> Self {
        let body = json!({"code": code.as_str(), "message": message, "hint": hint});
        Self {
            status,
            content_type: "application/json",
            body: Cow::Owned(body.to_string().into_bytes()),
            code: Some(code),
            authorised: false,
        }
    }

    fn of_code(status: u16, code: Code, message: &str) -> Self {
        Self::refused(status, code, message, &code.hint(&Fields::default()))
    }

    pub(crate) fn into_response(self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let mut headers = vec![("Content-Type", self.content_type)];
        headers.extend(HEADERS);
        headers.into_iter().fold(
            tiny_http::Response::from_data(self.body.into_owned()).with_status_code(self.status),
            |response, (name, value)| match tiny_http::Header::from_bytes(name, value) {
                Ok(header) => response.with_header(header),
                Err(()) => response,
            },
        )
    }
}

/// Answers `request` to the server on `port` whose token is `token` and
/// whose launch codes are `codes`, at `now`, drawing a binding's page with
/// `page`.
pub fn respond(
    request: &Request,
    port: u16,
    token: &Token,
    codes: &mut Codes,
    now: Instant,
    page: impl FnOnce(&LogicalPath) -> Result<Page, PageError>,
) -> Reply {
    if request.host.as_deref() != Some(format!("127.0.0.1:{port}").as_str()) {
        return Reply::of_code(
            403,
            Code::ServeHost,
            "the Host header does not name this server",
        );
    }
    let unknown = || {
        Reply::of_code(
            404,
            Code::ServeUnknownRoute,
            &format!(
                "the server does not answer {} {}",
                request.method, request.url
            ),
        )
    };
    let (path, query) = request
        .url
        .split_once('?')
        .unwrap_or((request.url.as_str(), ""));
    let cross_site = request.fetch_site.as_deref() == Some("cross-site")
        || request
            .origin
            .as_deref()
            .is_some_and(|origin| origin != format!("http://127.0.0.1:{port}"));
    if request.method == "POST" && cross_site {
        return Reply::of_code(
            403,
            Code::ServeOrigin,
            "the request comes from another site",
        );
    }
    match (request.method.as_str(), path) {
        ("GET", "/api/probe") => probe(query, token).unwrap_or_else(unknown),
        ("POST", "/api/session") => exchange(&request.body, token, codes, now),
        ("POST", "/api/codes") => {
            with_token(request, token, || make(&request.body, port, codes, now))
        }
        ("GET", _) if path.starts_with("/api/bindings/") => {
            with_token(request, token, || {
                match path.trim_start_matches("/api").parse::<LogicalPath>() {
                    Ok(binding) => draw(page(&binding)),
                    Err(_) => unknown(),
                }
            })
        }
        ("GET", _) => match APP.iter().find(|(route, ..)| *route == path) {
            Some((_, media_type, body)) => Reply::ok(media_type, Cow::Borrowed(body)),
            None => unknown(),
        },
        _ => unknown(),
    }
}

/// The reply `answer` gives, marked authorised, when the request carries
/// `token` as its bearer token; else a refusal.
fn with_token(request: &Request, token: &Token, answer: impl FnOnce() -> Reply) -> Reply {
    let presented = request
        .authorization
        .as_deref()
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|presented| !presented.is_empty());
    match presented {
        None => Reply::of_code(401, Code::ServeToken, "the request carries no token"),
        Some(presented) if !token.matches(presented) => Reply::of_code(
            401,
            Code::ServeToken,
            "the request's token is not this server's",
        ),
        Some(_) => Reply {
            authorised: true,
            ..answer()
        },
    }
}

/// The reply to `POST /api/codes`: a launch code for the binding the body
/// names, in a new opener whose path is the reply.
fn make(body: &str, port: u16, codes: &mut Codes, now: Instant) -> Reply {
    let binding = body
        .parse::<LogicalPath>()
        .ok()
        .filter(|path| path.as_str().starts_with("/bindings/"));
    let Some(binding) = binding else {
        let message = format!("{body:?} is not a path under /bindings");
        return Reply::of_code(422, Code::UiNotABinding, &message);
    };
    match codes.make(port, &binding, now) {
        Ok(opener) => Reply::ok(
            "text/plain",
            Cow::Owned(opener.as_os_str().as_encoded_bytes().to_vec()),
        ),
        Err(reason) => Reply::of_code(500, Code::ServeFailed, &reason),
    }
}

/// The reply to `POST /api/session`: the token, for a launch code in the
/// body that is known, unused and unexpired.
fn exchange(body: &str, token: &Token, codes: &mut Codes, now: Instant) -> Reply {
    match codes.spend(body, now) {
        Ok(()) => Reply {
            authorised: true,
            ..Reply::ok("text/plain", Cow::Owned(token.expose().as_bytes().to_vec()))
        },
        Err(code) => {
            let message = match code {
                Code::ServeCodeUsed => "the launch code was already used",
                Code::ServeCodeExpired => "the launch code expired",
                _ => "the launch code is not one this server made",
            };
            Reply::of_code(401, code, message)
        }
    }
}

/// The reply to a probe: the proof for its nonce, which is 16 to 128 hex
/// digits.
fn probe(query: &str, token: &Token) -> Option<Reply> {
    let nonce = query.strip_prefix("nonce=")?;
    let valid = (16..=128).contains(&nonce.len()) && nonce.bytes().all(|b| b.is_ascii_hexdigit());
    valid.then(|| Reply::ok("text/plain", Cow::Owned(token.prove(nonce).into_bytes())))
}

fn draw(page: Result<Page, PageError>) -> Reply {
    match page {
        Ok(page) => match serde_json::to_vec(&page) {
            Ok(body) => Reply::ok("application/json", Cow::Owned(body)),
            Err(error) => Reply::of_code(500, Code::ServeFailed, &error.to_string()),
        },
        Err(error) => {
            let refusal = error.refusal();
            let status = if error.code() == Code::PathNotFound {
                404
            } else {
                422
            };
            Reply::refused(
                status,
                error.code(),
                &refusal.message,
                refusal.hint.as_deref().unwrap_or_default(),
            )
        }
    }
}
