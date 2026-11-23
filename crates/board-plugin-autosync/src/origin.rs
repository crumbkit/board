//! The checks before a board is fetched: that it is a checkout of its own,
//! not inside another, and that every URL its branch's remote pushes to is
//! one the owner signed. git runs only through the host.

use std::path::Path;
use std::time::Duration;

use board_ops::{Host, Run};

/// How long one quick git command may take.
const GIT_TIME: Duration = Duration::from_secs(30);

/// What a git command printed, and how it ended.
#[derive(Debug)]
pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
}

impl Ran {
    pub(crate) fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs `git -C <at> <arguments>`. A git that does not start or finish is
/// `None`, which every caller takes as a failure.
pub(crate) fn git(host: &dyn Host, at: &Path, arguments: &[&str]) -> Option<Ran> {
    let at = at.to_string_lossy();
    let arguments: Vec<String> = ["-C", &at]
        .into_iter()
        .chain(arguments.iter().copied())
        .map(str::to_owned)
        .collect();
    let exited = host
        .run(&Run {
            program: "git",
            arguments: &arguments,
            directory: None,
            input: &[],
            deadline: GIT_TIME,
            added: &[],
        })
        .ok()?;
    Some(Ran {
        code: exited.code,
        stdout: String::from_utf8_lossy(&exited.stdout).into_owned(),
    })
}

/// Why a board is refused before it is fetched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unfit {
    /// The directory is not the top of a git checkout, or lies inside
    /// another checkout.
    Nested,
    /// The branch follows no remote, the remote has no push URL, or a push
    /// URL is not in the record's origins: each URL not allowed, with any
    /// credentials dropped.
    NotAllowed { urls: Vec<String> },
}

/// Refuses `path` unless it is the top of its own git checkout and no
/// checkout holds its parent.
pub(crate) fn own_checkout(host: &dyn Host, path: &Path) -> Result<(), Unfit> {
    let top = git(host, path, &["rev-parse", "--show-toplevel"]).filter(Ran::ok);
    let canonical = path.canonicalize().ok();
    let same = top.is_some_and(|top| Path::new(top.stdout.trim()).canonicalize().ok() == canonical);
    let parent = path.parent().unwrap_or(path);
    let inside = git(host, parent, &["rev-parse", "--show-toplevel"]).is_some_and(|ran| ran.ok());
    if same && !inside {
        Ok(())
    } else {
        Err(Unfit::Nested)
    }
}

/// Refuses `path` unless every push URL of the remote `branch` follows, as
/// git gives it with `pushInsteadOf` applied, is one of `origins` once both
/// are normalised.
pub(crate) fn allowed(
    host: &dyn Host,
    path: &Path,
    branch: &str,
    origins: &[String],
) -> Result<(), Unfit> {
    let key = format!("branch.{branch}.remote");
    let remote = git(host, path, &["config", "--get", &key]).filter(Ran::ok);
    let Some(remote) = remote.map(|ran| ran.stdout.trim().to_owned()) else {
        return Err(Unfit::NotAllowed { urls: Vec::new() });
    };
    let urls = git(host, path, &["remote", "get-url", "--push", "--all", &remote]).filter(Ran::ok);
    let urls: Vec<&str> = urls
        .as_ref()
        .map(|ran| ran.stdout.lines().filter(|line| !line.is_empty()).collect())
        .unwrap_or_default();
    let allowed: Vec<String> = origins.iter().map(|origin| normalised(origin)).collect();
    let refused: Vec<String> = (urls.iter())
        .map(|url| normalised(url))
        .filter(|url| !allowed.contains(url))
        .collect();
    if urls.is_empty() || !refused.is_empty() {
        return Err(Unfit::NotAllowed { urls: refused });
    }
    Ok(())
}

/// `url` in one form for comparison: no scheme, no user or password, the
/// host in lower case, and no `.git` suffix or trailing slash, so
/// `git@host:o/r.git`, `ssh://git@host/o/r` and `https://user:token@host/o/r/`
/// are all `host/o/r`. A local path keeps its path.
pub(crate) fn normalised(url: &str) -> String {
    let url = url.trim();
    let joined = if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if scheme == "file" {
            format!("/{path}")
        } else {
            let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
            format!("{}/{path}", host.to_lowercase())
        }
    } else if let Some((authority, path)) = scp_like(url) {
        let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
        format!("{}/{path}", host.to_lowercase())
    } else {
        url.to_owned()
    };
    let trimmed = joined.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    trimmed.trim_end_matches('/').replace("//", "/")
}

/// `[user@]host:path`, git's scp-like form: a colon before any slash, in a
/// URL that is not a local path.
fn scp_like(url: &str) -> Option<(&str, &str)> {
    if url.starts_with('/') || url.starts_with('.') {
        return None;
    }
    let (authority, path) = url.split_once(':')?;
    (!authority.contains('/')).then_some((authority, path.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::normalised;

    #[test]
    fn the_forms_of_one_remote_normalise_to_one_url_and_another_remote_does_not() {
        let one = [
            "git@github.com:crumbkit/board.git",
            "ssh://git@github.com/crumbkit/board",
            "https://github.com/crumbkit/board.git/",
            "https://someone:secret-token@GitHub.com/crumbkit/board",
            "github.com:crumbkit/board",
        ];
        for url in one {
            assert_eq!(normalised(url), "github.com/crumbkit/board", "{url}");
        }
        assert_ne!(
            normalised("git@github.com:someone/board.git"),
            "github.com/crumbkit/board"
        );
        assert_ne!(
            normalised("git@evil.example:crumbkit/board.git"),
            "github.com/crumbkit/board"
        );
    }

    #[test]
    fn a_local_path_keeps_its_path_without_a_trailing_slash_or_suffix() {
        assert_eq!(normalised("/tmp/remote.git/"), "/tmp/remote");
        assert_eq!(normalised("file:///tmp/remote"), "/tmp/remote");
        assert_ne!(normalised("/tmp/remote"), normalised("/tmp/other"));
    }

    #[test]
    fn credentials_never_survive_normalising() {
        let url = normalised("https://user:hunter2@example.com/o/r");
        assert!(!url.contains("hunter2") && !url.contains("user"), "{url}");
    }
}
