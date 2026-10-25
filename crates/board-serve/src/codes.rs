//! Launch codes (STANDARDS.md 28). `board ui open` asks the server for a
//! code, and the server writes an opener page that carries it in the URL's
//! fragment. The page posts the code once, to `/api/session`, for the token.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use board_model::{Code, LogicalPath};
use sha2::{Digest as _, Sha256};

use crate::Token;

/// How long a launch code lasts.
pub(crate) const CODE_LIFE: Duration = Duration::from_mins(1);

/// How long the digest of a used or expired code is kept, so a late
/// exchange is told which it was.
const REMEMBER: Duration = Duration::from_mins(10);

/// One code the server made: only its digest, when it was made, the opener
/// that holds it until it is used or expires, and whether it was used.
#[derive(Debug)]
struct Launch {
    digest: Vec<u8>,
    made: Instant,
    opener: Option<PathBuf>,
    used: bool,
}

/// The launch codes of one server, whose openers go in the cache of the
/// board at `root`.
#[derive(Debug)]
pub struct Codes {
    cache: PathBuf,
    root: PathBuf,
    launches: Vec<Launch>,
}

impl Codes {
    pub fn new(cache: &Path, root: &Path) -> Self {
        Self {
            cache: cache.to_owned(),
            root: root.to_owned(),
            launches: Vec::new(),
        }
    }

    /// Makes a code for the page of `binding` on the server at `port`,
    /// writes the opener that holds it, and returns the opener's path.
    pub(crate) fn make(
        &mut self,
        port: u16,
        binding: &LogicalPath,
        now: Instant,
    ) -> Result<PathBuf, String> {
        let code = Token::new().map_err(|error| error.to_string())?;
        let page = opener(port, code.expose(), binding);
        let opener = board_index::write_opener(&self.cache, &self.root, &page, CODE_LIFE)
            .map_err(|error| format!("the opener page cannot be written: {error}"))?;
        self.launches.push(Launch {
            digest: Sha256::digest(code.expose()).to_vec(),
            made: now,
            opener: Some(opener.clone()),
            used: false,
        });
        Ok(opener)
    }

    /// Spends `code`, which then never works again, and removes its opener.
    pub(crate) fn spend(&mut self, code: &str, now: Instant) -> Result<(), Code> {
        let digest = Sha256::digest(code);
        let launch = self
            .launches
            .iter_mut()
            .find(|launch| constant_time_eq::constant_time_eq(&launch.digest, &digest))
            .ok_or(Code::ServeCodeUnknown)?;
        if launch.used {
            return Err(Code::ServeCodeUsed);
        }
        if now.duration_since(launch.made) > CODE_LIFE {
            return Err(Code::ServeCodeExpired);
        }
        launch.used = true;
        remove(launch.opener.take());
        Ok(())
    }

    /// Removes the openers of codes that expired, and forgets old digests.
    pub(crate) fn expire(&mut self, now: Instant) {
        for launch in &mut self.launches {
            if now.duration_since(launch.made) > CODE_LIFE {
                remove(launch.opener.take());
            }
        }
        self.launches
            .retain(|launch| now.duration_since(launch.made) <= REMEMBER);
    }

    /// Removes every opener still held, as the server exits.
    pub(crate) fn close(&mut self) {
        for launch in &mut self.launches {
            remove(launch.opener.take());
        }
    }
}

fn remove(opener: Option<PathBuf>) {
    // An opener left behind holds a spent code, and the next one written
    // removes it.
    if let Some(opener) = opener {
        let _ = board_index::remove_opener(&opener);
    }
}

/// The opener page: it sends the browser to the page of `path` on the
/// server at `port`, with the launch code in the fragment, which no request
/// carries.
pub(crate) fn opener(port: u16, code: &str, path: &LogicalPath) -> Vec<u8> {
    let url = format!("http://127.0.0.1:{port}/#code={code}&{path}");
    let escaped = url
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<meta name=\"referrer\" content=\"no-referrer\">\n<meta http-equiv=\"refresh\" content=\"0;url={escaped}\">\n<title>Board</title>\n"
    )
    .into_bytes()
}
