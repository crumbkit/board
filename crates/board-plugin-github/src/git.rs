//! Running git in the checkout Board was started in: its root, its branch,
//! what a push would send, the push itself, and its upstream.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use board_plugin::child::ProcessError;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::hook::{Handed, PASSED};

/// How long one git command may run. A push or ls-remote reaches the
/// network.
const TIMEOUT: Duration = Duration::from_mins(5);
/// How much of git's stderr a failure's message carries, in characters.
pub(crate) const TAIL: usize = 2000;
/// Variables only the boundary's judged rules need, kept from git, its
/// hooks, ssh and gh.
pub(crate) const MODEL_VARIABLES: [&str; 2] = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];
/// The user and password of a URL with a scheme, which a message never
/// quotes.
static USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"://[^/@\s]+@").expect("the pattern is a constant"));

/// `text` with the user and password removed from every URL in it.
pub(crate) fn shown(text: &str) -> String {
    USERINFO.replace_all(text, "://").into_owned()
}

/// The end of `text`, at most [`TAIL`] characters.
pub(crate) fn tail(text: &str) -> String {
    let skip = text.chars().count().saturating_sub(TAIL);
    text.chars().skip(skip).collect()
}

/// A finished git command: its exit code, 0 for success, and what it
/// printed.
pub(crate) struct Ran {
    pub(crate) code: i32,
    /// What it printed, each byte that is not UTF-8 replaced by U+FFFD.
    pub(crate) stdout: String,
    /// Whether stdout was UTF-8, so nothing was replaced.
    pub(crate) utf8: bool,
    pub(crate) stderr: String,
}

/// The finished `git -C <at> <arguments>`, reading `stdin`; one that did not
/// finish exits 1 saying why.
pub(crate) fn run(host: &dyn Host, at: &Path, arguments: &[&str], stdin: &str) -> Ran {
    run_adding(host, at, arguments, stdin, &[])
}

/// [`run`] with the variables `added` set for git and the hooks it runs.
fn run_adding(
    host: &dyn Host,
    at: &Path,
    arguments: &[&str],
    stdin: &str,
    added: &[(&str, &str)],
) -> Ran {
    let at = at.to_string_lossy();
    let arguments: Vec<String> = ["-C", &at]
        .into_iter()
        .chain(arguments.iter().copied())
        .map(str::to_owned)
        .collect();
    let ran = host.run_hiding(
        &Run {
            program: "git",
            arguments: &arguments,
            directory: None,
            input: stdin.as_bytes(),
            deadline: TIMEOUT,
            added,
        },
        &MODEL_VARIABLES,
    );
    match ran {
        Ok(exited) => Ran {
            code: exited.code.unwrap_or(1),
            stdout: String::from_utf8_lossy(&exited.stdout).into_owned(),
            utf8: std::str::from_utf8(&exited.stdout).is_ok(),
            stderr: String::from_utf8_lossy(&exited.stderr).into_owned(),
        },
        Err(error) => Ran {
            code: 1,
            stdout: String::new(),
            utf8: true,
            stderr: format!("it {}", unfinished(&error)),
        },
    }
}

/// Why a process did not finish, as a phrase after its name.
pub(crate) fn unfinished(error: &ProcessError) -> String {
    match error {
        ProcessError::NotStarted { source } => format!("did not start: {source}"),
        ProcessError::Timeout { .. } => "did not finish: it ran past its deadline".to_owned(),
        ProcessError::TooLarge { limit, .. } => {
            format!("did not finish: it printed more than {limit} bytes")
        }
    }
}

/// What `git <arguments>` prints in `at`, or `github::git::failed` naming
/// the command and git's error. Output that is not UTF-8 is
/// `github::git::not_utf8`, since it could not be checked as git gives it.
pub(crate) fn git(
    host: &dyn Host,
    at: &Path,
    arguments: &[&str],
    stdin: &str,
) -> Result<String, PluginError> {
    let (printed, utf8) = lossy(host, at, arguments, stdin)?;
    if utf8 {
        return Ok(printed);
    }
    let command = named(arguments);
    Err(error(
        "github::git::not_utf8",
        format!(
            "`{command}` printed bytes that are not UTF-8, so the boundary cannot check them as \
             git would send them; save the commit message, tag or file it shows as UTF-8"
        ),
        json!({"command": command}),
    ))
}

/// What `git <arguments>` prints in `at`, each byte that is not UTF-8
/// replaced by U+FFFD, and whether none was; or `github::git::failed`
/// naming the command and git's error.
pub(crate) fn lossy(
    host: &dyn Host,
    at: &Path,
    arguments: &[&str],
    stdin: &str,
) -> Result<(String, bool), PluginError> {
    let ran = run(host, at, arguments, stdin);
    if ran.code == 0 {
        return Ok((ran.stdout, ran.utf8));
    }
    let command = named(arguments);
    Err(error(
        "github::git::failed",
        format!(
            "`{command}` exited {}: {}; check git and the checkout",
            ran.code,
            shown(&tail(ran.stderr.trim()))
        ),
        json!({"command": command}),
    ))
}

/// `git` and its first two arguments after any `-c <name>=<value>`, which
/// name a command in a message.
fn named(arguments: &[&str]) -> String {
    let mut rest = arguments;
    while let ["-c", _, after @ ..] = rest {
        rest = after;
    }
    ["git"]
        .into_iter()
        .chain(rest.iter().copied().take(2))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A refusal with `code`, `message` and the fields of `fields`, an object.
pub(crate) fn error(code: &str, message: String, fields: Value) -> PluginError {
    let fields = match fields {
        Value::Object(fields) => fields,
        _ => Map::new(),
    };
    PluginError {
        code: code.to_owned(),
        message,
        fields,
        hint: None,
    }
}

/// The root of the git checkout Board was started in, which Board names in
/// `BOARD_CALLER_DIR`, or `github::checkout::not_found`.
pub(crate) fn top(host: &dyn Host) -> Result<PathBuf, PluginError> {
    let directory = host.settings().caller.clone();
    let ran = (directory.as_deref()).map(|at| run(host, at, &["rev-parse", "--show-toplevel"], ""));
    match (directory, ran) {
        (Some(_), Some(ran)) if ran.code == 0 => Ok(PathBuf::from(ran.stdout.trim())),
        (directory, _) => {
            let named = directory.map(|at| at.display().to_string());
            Err(error(
                "github::checkout::not_found",
                format!(
                    "Board was started in {}, which is not in a git checkout; run the command \
                     from inside the checkout to send from",
                    named.as_deref().unwrap_or("no directory it named")
                ),
                json!({"directory": named.unwrap_or_default()}),
            ))
        }
    }
}

/// The value of variable `name` among the capability's declared ones.
pub(crate) fn variable(host: &dyn Host, name: &str) -> Option<String> {
    (host.settings().environment.0.iter())
        .find(|(variable, _)| variable == name)
        .map(|(_, value)| value.to_string_lossy().into_owned())
}

/// The branch checked out and the commit it is at, resolved once, or
/// `github::checkout::detached`.
pub(crate) fn branch(host: &dyn Host, at: &Path) -> Result<(String, String), PluginError> {
    let name = git(host, at, &["branch", "--show-current"], "")?
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(error(
            "github::checkout::detached",
            "the checkout is on no branch, so there is nothing to push; run \
             `git switch -c <branch>` first"
                .to_owned(),
            json!({}),
        ));
    }
    let head = git(host, at, &["rev-parse", "--verify", "HEAD^{commit}"], "")?;
    Ok((name, head.trim().to_owned()))
}

/// The commits the refs on `remote` name, as `git ls-remote` lists them:
/// one network round trip.
pub(crate) fn advertised(
    host: &dyn Host,
    at: &Path,
    remote: &str,
) -> Result<Vec<String>, PluginError> {
    let listed = git(host, at, &["ls-remote", remote], "")?;
    Ok((listed.split_whitespace().step_by(2))
        .map(str::to_owned)
        .collect())
}

/// The commits a push of `head` would send, oldest first: those no commit
/// in `have` reaches. A ref on the remote that names a commit the checkout
/// lacks cannot reach `head`, so `--ignore-missing` leaves it out.
pub(crate) fn outgoing(
    host: &dyn Host,
    at: &Path,
    head: &str,
    have: &[String],
) -> Result<Vec<String>, PluginError> {
    let stdin: String = have.iter().flat_map(|sha| ["^", sha, "\n"]).collect();
    let arguments = ["rev-list", "--reverse", "--ignore-missing", "--stdin", head];
    let listed = git(host, at, &arguments, &stdin)?;
    Ok(listed.split_whitespace().map(str::to_owned).collect())
}

/// Pushes `head` to the branch `name` on origin: git's porcelain line for
/// it, or `github::push::failed`. Only the checked commit goes: no tags, no
/// submodule commits and no push options, whatever git's config says. The
/// pre-push hook is handed the check that passed, in `BOARD_GITHUB_PASSED`,
/// so it reuses that check in place of judging the same parts again.
pub(crate) fn push(
    host: &dyn Host,
    at: &Path,
    name: &str,
    head: &str,
    handed: &Handed,
) -> Result<String, PluginError> {
    let digest = handed.digest.as_str();
    let handed = serde_json::to_string(handed).expect("a check handed on is strings");
    let refspec = format!("{head}:refs/heads/{name}");
    let arguments = [
        "-c",
        "push.pushOption=",
        "push",
        "--porcelain",
        "--no-follow-tags",
        "--recurse-submodules=no",
        "origin",
        &refspec,
    ];
    let ran = run_adding(host, at, &arguments, "", &[(PASSED, &handed)]);
    let marker = format!("\t{refspec}\t");
    let line = (ran.stdout.lines()).find(|line| line.contains(&marker));
    match line {
        Some(line) if ran.code == 0 => Ok(line.split('\t').skip(1).collect::<Vec<_>>().join(" ")),
        _ => {
            let printed = if ran.stderr.trim().is_empty() {
                ran.stdout.trim()
            } else {
                ran.stderr.trim()
            };
            Err(error(
                "github::push::failed",
                format!(
                    "`git push origin {name}` failed, and may have pushed if it did not finish: \
                     {}; fix what git says, such as pulling first, and run it again",
                    shown(&tail(printed))
                ),
                json!({"branch": name, "digest": digest}),
            ))
        }
    }
}

/// Makes origin's branch the branch's upstream as `git push -u` does, which
/// needs no remote-tracking ref, so it works in a single-branch clone.
pub(crate) fn track(host: &dyn Host, at: &Path, name: &str) -> Result<(), PluginError> {
    let remote = format!("branch.{name}.remote");
    let merge = format!("branch.{name}.merge");
    let head = format!("refs/heads/{name}");
    git(host, at, &["config", &remote, "origin"], "")?;
    git(host, at, &["config", &merge, &head], "")?;
    Ok(())
}
