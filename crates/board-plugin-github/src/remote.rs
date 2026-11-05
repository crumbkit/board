//! The one rule every sending action, the pre-push hook and `covers` read a
//! git URL by: which URLs reach GitHub, which repository they name, and
//! which cannot be read.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use regex::Regex;
use serde_json::json;

use crate::git::{HIDDEN_FROM_GIT, error, git, run, shown, variable};

/// How long `ssh -G` may run.
const SSH_TIMEOUT: Duration = Duration::from_secs(10);

/// Compiles a constant pattern.
fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("the pattern is a constant")
}

/// The hosts that serve GitHub's git.
static GITHUB_HOSTS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(?:www\.|ssh\.)?github\.com$"));
/// The `github.com-<alias>` ssh hosts kept for several accounts, which count
/// as GitHub over ssh alone.
static GITHUB_ALIAS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^github\.com-[\w.-]+$"));
/// A host name ssh may be asked about: no space, quote, `%` or other
/// character an ssh config could expand.
static PLAIN_HOST: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z0-9_][A-Za-z0-9._-]*$"));
/// A URL with a scheme, as scheme, host and path, any user and port left
/// out.
static SCHEMED: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"^([A-Za-z][A-Za-z0-9+.-]*)://(?:[^@/]*@)?(\[[^\]/]*\]|[^:/]*)(?::\d*)?(/.*)?$")
});
/// Git's scp-like ssh form, `[user@]host:path`, whose host holds no slash,
/// or is in brackets with any port.
static SCP: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(?:[^@/:]*@)?(\[[^\]/]*\]|[^@/:]+):(.*)$"));
/// A GitHub repository's path: owner and repository, with an optional
/// `.git` and trailing slash.
static REPOSITORY: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^/?([A-Za-z0-9-]+)/([A-Za-z0-9._-]+?)(?:\.git)?/?$"));
/// The schemes git reaches a host over ssh by.
const SSH: [&str; 3] = ["ssh", "git+ssh", "ssh+git"];
/// The variables that make git run an ssh command of its own.
pub(crate) const SSH_VARIABLES: [&str; 2] = ["GIT_SSH_COMMAND", "GIT_SSH"];

/// How the checkout's git resolves an ssh host to the host it connects to.
pub(crate) enum Resolver<'a> {
    /// `ssh -G`, which reads ssh's config and opens no connection.
    Ssh(&'a dyn Host),
    /// Git runs another ssh command, which may resolve hosts differently,
    /// so no host is resolved.
    Custom,
}

impl Resolver<'_> {
    /// The host ssh connects to for `host`, lower case, or none.
    fn resolve(&self, host: &str) -> Option<String> {
        match self {
            Self::Ssh(runner) => ssh_hostname(*runner, host),
            Self::Custom => None,
        }
    }
}

/// How the checkout at `at` resolves an ssh host: `ssh -G`, or nothing when
/// git runs another ssh command through `GIT_SSH_COMMAND`, `GIT_SSH` or
/// `core.sshCommand`, which may pass options a plain `ssh -G` does not see.
pub(crate) fn resolver<'a>(host: &'a dyn Host, at: &Path) -> Resolver<'a> {
    let named = SSH_VARIABLES
        .iter()
        .any(|name| variable(host, name).is_some_and(|value| !value.is_empty()));
    let configured = run(host, at, &["config", "--get", "core.sshCommand"], "");
    if named || !configured.stdout.trim().is_empty() {
        Resolver::Custom
    } else {
        Resolver::Ssh(host)
    }
}

/// The host ssh connects to for `host`, from `ssh -G`. Only a plain host
/// name is asked about, so no host from a checkout's config reaches an ssh
/// config's `Match exec` as shell text; any other is none.
pub(crate) fn ssh_hostname(runner: &dyn Host, host: &str) -> Option<String> {
    if !PLAIN_HOST.is_match(host) {
        return None;
    }
    let arguments = ["-G", "--", host].map(str::to_owned);
    let run = Run {
        program: "ssh",
        arguments: &arguments,
        directory: None,
        input: b"",
        deadline: SSH_TIMEOUT,
        added: &[],
    };
    let exited = runner.run_hiding(&run, &HIDDEN_FROM_GIT).ok()?;
    if exited.code != Some(0) {
        return None;
    }
    let stdout = String::from_utf8_lossy(&exited.stdout);
    let name = stdout
        .lines()
        .find_map(|line| line.strip_prefix("hostname "));
    name.map(|name| name.trim().to_lowercase())
}

/// The owner and repository, in lower case, of the GitHub repository `url`
/// reaches, or none for another place. A local path, a `file://` URL and a
/// host other than GitHub's are none. An ssh host that is not GitHub's by
/// name is resolved through ssh's config, so a `Host` alias of github.com
/// counts. A URL that could reach GitHub but names no repository this rule
/// reads, such as an alias ssh cannot resolve or a `<transport>::<address>`
/// remote helper, is `github::remote::unknown`.
pub(crate) fn github_repository(
    url: &str,
    resolver: &Resolver<'_>,
) -> Result<Option<(String, String)>, PluginError> {
    let local = url.starts_with(['/', '.', '~']);
    let schemed = SCHEMED.captures(url);
    let scp = (schemed.is_none() && !url.contains("::") && !local)
        .then(|| SCP.captures(url))
        .flatten();
    let found = if let Some(found) = &schemed {
        let path = found.get(3).map_or("", |path| path.as_str());
        Some((found[1].to_lowercase(), found[2].to_lowercase(), path))
    } else if let Some(found) = &scp {
        Some((
            "ssh".to_owned(),
            found[1].to_lowercase(),
            found.get(2).map_or("", |path| path.as_str()),
        ))
    } else if url.contains("::") {
        None
    } else {
        return Ok(None);
    };
    let found = match found {
        Some((scheme, _, _)) if scheme == "file" => return Ok(None),
        Some((scheme, host, path)) => {
            let host = host_of(&host);
            let ssh = SSH.contains(&scheme.as_str());
            let host = if ssh && !GITHUB_HOSTS.is_match(&host) && !GITHUB_ALIAS.is_match(&host) {
                resolver
                    .resolve(&host)
                    .map(|name| name.strip_suffix('.').unwrap_or(&name).to_owned())
            } else {
                Some(host)
            };
            match host {
                Some(host)
                    if !(GITHUB_HOSTS.is_match(&host) || ssh && GITHUB_ALIAS.is_match(&host)) =>
                {
                    return Ok(None);
                }
                Some(_) => Some(path),
                None => None,
            }
        }
        None => None,
    };
    let Some(matched) = found.and_then(|path| REPOSITORY.captures(path)) else {
        let url = shown(url);
        return Err(error(
            "github::remote::unknown",
            format!(
                "{url} could reach GitHub, but it names no repository the plugin can read, so \
                 what it sends cannot be checked; use a URL of the form \
                 https://github.com/<owner>/<repo>.git or git@github.com:<owner>/<repo>.git"
            ),
            json!({"url": url}),
        ));
    };
    Ok(Some((matched[1].to_lowercase(), matched[2].to_lowercase())))
}

/// A URL's host as git hands it to ssh: without the brackets, and any port
/// in them, and without a trailing dot, which names the same host.
fn host_of(host: &str) -> String {
    let host = (host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']')))
    .unwrap_or(host);
    let host = match host.split_once(':') {
        Some((name, port)) if !port.contains(':') => name,
        _ => host,
    };
    host.strip_suffix('.').unwrap_or(host).to_owned()
}

/// The owner and repository, in lower case, of the GitHub repository the
/// checkout's origin names. This reads the URL as configured, before any
/// `insteadOf` rewrite, so it names the repository the person wrote (a
/// soft limit: a rewrite to another host still checks as that repository).
/// `insteadOf` moves fetch and push alike, so the commits checked are still
/// those the push sends; a push URL that differs from the fetch URL is
/// `github::remote::split`.
pub(crate) fn repository(host: &dyn Host, at: &Path) -> Result<(String, String), PluginError> {
    let url = run(host, at, &["config", "--get", "remote.origin.url"], "");
    let url = url.stdout.trim();
    let Some(matched) = github_repository(url, &resolver(host, at))? else {
        let origin = shown(url);
        let named = if origin.is_empty() {
            "not set"
        } else {
            &origin
        };
        return Err(error(
            "github::remote::not_github",
            format!(
                "the checkout's origin is {named}, which is not a GitHub repository; set it with \
                 `git remote set-url origin https://github.com/<owner>/<repo>.git`"
            ),
            json!({"origin": origin}),
        ));
    };
    let fetch = git(host, at, &["remote", "get-url", "origin"], "")?;
    let fetch = fetch.trim();
    let pushes = git(
        host,
        at,
        &["remote", "get-url", "--push", "--all", "origin"],
        "",
    )?;
    let pushes: Vec<&str> = pushes.split_whitespace().collect();
    if pushes != [fetch] {
        let shown_pushes: Vec<String> = pushes.iter().map(|url| shown(url)).collect();
        return Err(error(
            "github::remote::split",
            format!(
                "origin fetches from {} but pushes to {}, so the commits checked would not be \
                 those sent; push from a checkout whose origin fetches and pushes to the same \
                 repository, such as after `git config --unset-all remote.origin.pushurl`",
                shown(fetch),
                shown_pushes.join(", ")
            ),
            json!({"fetch": shown(fetch), "push": shown_pushes}),
        ));
    }
    Ok(matched)
}

/// How a write leaves the checkout, which decides the URLs it goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Via {
    /// `git push`, which goes to the remote's push URLs, else its URLs.
    Push,
    /// gh, which goes where the remote's fetch URL points.
    Gh,
}

/// The GitHub destinations, `github:<owner>/<repo>` in lower case and
/// sorted, of a push git makes to `remote` at `url`. Git names the remote,
/// or the URL it was given in its place, and the URL it pushes to, after any
/// rewrite. Each is read, and so are the remote's push URLs as configured,
/// else its URLs. A gh write goes where the fetch URL points, so with
/// [`Via::Gh`] only the remote's URLs are read. A URL that could reach
/// GitHub but cannot be read is `github::remote::unknown`, so the hook
/// fails closed.
pub(crate) fn destinations(
    host: &dyn Host,
    at: &Path,
    remote: &str,
    url: &str,
    via: Via,
) -> Result<Vec<String>, PluginError> {
    let listed = |key: String| {
        let ran = run(host, at, &["config", "--get-all", &key], "");
        (ran.stdout.split_whitespace())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let mut configured = if via == Via::Push {
        listed(format!("remote.{remote}.pushurl"))
    } else {
        Vec::new()
    };
    if configured.is_empty() {
        configured = listed(format!("remote.{remote}.url"));
    }
    let named: Vec<&str> = if configured.is_empty() {
        vec![url, remote]
    } else {
        [url]
            .into_iter()
            .chain(configured.iter().map(String::as_str))
            .collect()
    };
    let resolver = resolver(host, at);
    let mut found = BTreeSet::new();
    for each in named.into_iter().filter(|each| !each.is_empty()) {
        if let Some((owner, repo)) = github_repository(each, &resolver)? {
            found.insert(format!("github:{owner}/{repo}"));
        }
    }
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests;
