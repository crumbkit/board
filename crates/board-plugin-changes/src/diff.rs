//! Running git in a mounted repository, the files a change edits, and those
//! no planned path matches.
//!
//! A change's diff runs from where its head left the branch the mount
//! follows to its head, so it holds every commit of the change and none the
//! branch gained since. git runs without the user's or the system's config,
//! so an alias, an external diff driver or a rename setting cannot change
//! which files it lists.

use std::path::Path;
use std::time::Duration;

use board_ops::{Host, PluginError, Run, literal};
use board_plugin::child::ProcessError;
use serde_json::Value;

use crate::board::{self, rows, text};

/// The variables that keep the user's and the system's git config out of a
/// git command, and stop it prompting.
const ISOLATED: [(&str, &str); 3] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
];
/// How long the diff may take, well inside the 30 s Board gives `next` and
/// `advance`.
const DIFF_DEADLINE: Duration = Duration::from_secs(20);
/// How much of git's stderr a failure carries.
const TAIL: usize = 4096;

/// Why a git command gave no output. `started` is false when git could not
/// be run at all; `timed_out` is true when it ran past its deadline.
#[derive(Debug)]
pub(crate) struct Failed {
    pub(crate) why: String,
    pub(crate) started: bool,
    pub(crate) timed_out: bool,
}

/// What `git <arguments>` prints in `repository`, run with `ISOLATED` for at
/// most `deadline`, or why it failed.
pub(crate) fn git(
    host: &dyn Host,
    repository: &Path,
    arguments: &[&str],
    deadline: Duration,
) -> Result<String, Failed> {
    let command = arguments.first().copied().unwrap_or("git");
    let owned: Vec<String> = arguments.iter().map(|each| (*each).to_owned()).collect();
    let run = Run {
        program: "git",
        arguments: &owned,
        directory: Some(repository),
        input: b"",
        deadline,
        added: &ISOLATED,
    };
    let shown = repository.display();
    match host.run(&run) {
        Ok(exited) if exited.code == Some(0) => {
            Ok(String::from_utf8_lossy(&exited.stdout).into_owned())
        }
        Ok(exited) => {
            let stderr = String::from_utf8_lossy(&exited.stderr);
            let stderr = stderr.trim();
            let start = stderr.len().saturating_sub(TAIL);
            let start = (start..stderr.len())
                .find(|at| stderr.is_char_boundary(*at))
                .unwrap_or(stderr.len());
            Err(Failed {
                why: format!("`git {command}` failed in {shown}: {}", &stderr[start..]),
                started: true,
                timed_out: false,
            })
        }
        Err(ProcessError::Timeout { .. }) => Err(Failed {
            why: format!(
                "`git {command}` timed out after {} s in {shown}",
                deadline.as_secs()
            ),
            started: true,
            timed_out: true,
        }),
        Err(ProcessError::NotStarted { source }) if !repository.is_dir() => Err(Failed {
            why: format!("`git {command}` cannot run in {shown}: {source}"),
            started: true,
            timed_out: false,
        }),
        Err(ProcessError::NotStarted { source }) => Err(Failed {
            why: format!("git cannot be run: {source}; put git on PATH"),
            started: false,
            timed_out: false,
        }),
        Err(ProcessError::TooLarge { limit, .. }) => Err(Failed {
            why: format!("`git {command}` printed more than {limit} bytes in {shown}"),
            started: true,
            timed_out: false,
        }),
    }
}

/// Each file the change at `head` adds, changes or deletes in `mount`'s
/// repository, by logical path, or why it cannot be read.
///
/// The diff runs from the merge base of `head` and the branch the mount
/// follows, which Board reports in the location it resolves `mount` to. A
/// rename counts as deleting one path and adding another, so both must be
/// planned.
pub(crate) fn edited(
    host: &dyn Host,
    mount: &str,
    head: &str,
) -> Result<Result<Vec<String>, String>, PluginError> {
    let Some(answer) = board::resolve(host, mount)? else {
        return Ok(Err(format!(
            "nothing is at {mount}, so the change's diff cannot be read; check the mount with \
             `board ls /repos`"
        )));
    };
    let location = &answer.location;
    let Some(follow) = location
        .pointers
        .last()
        .and_then(|pointer| pointer.follow.as_deref())
    else {
        return Ok(Err(format!(
            "{mount} is not a mount, so it follows no branch to diff from; set the change's mount \
             to a mount"
        )));
    };
    let arguments = [
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        "--merge-base",
        "--end-of-options",
        follow,
        head,
    ];
    let names = match git(host, &location.board.repository, &arguments, DIFF_DEADLINE) {
        Ok(names) => names,
        Err(failed) => return Ok(Err(unread(failed, head, follow))),
    };
    let prefix = if location.path.is_empty() {
        String::new()
    } else {
        format!("{}/", location.path)
    };
    let mount = mount.trim_end_matches('/');
    Ok(Ok((names.split('\0'))
        .filter(|name| !name.is_empty())
        .filter_map(|name| name.strip_prefix(&prefix))
        .map(|name| format!("{mount}/{name}"))
        .collect()))
}

/// Why the diff of `head` from `follow` could not be read: only a git that
/// ran and failed is told to fetch the commits.
fn unread(failed: Failed, head: &str, follow: &str) -> String {
    if !failed.started || failed.timed_out {
        return failed.why;
    }
    format!(
        "{}; fetch {head} and {follow} into the mounted repository",
        failed.why
    )
}

/// The files no planned path matches, compared with SQLite's GLOB as
/// controls match their paths.
pub(crate) fn outside(
    host: &dyn Host,
    files: &[String],
    planned: &[String],
) -> Result<Vec<String>, PluginError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let listed = |each: &[String]| literal(&Value::from(each.to_vec()).to_string());
    let sql = format!(
        "SELECT f.value FROM json_each({}) AS f WHERE NOT EXISTS (SELECT 1 FROM json_each({}) AS \
         p WHERE f.value GLOB p.value) ORDER BY f.key",
        listed(files),
        listed(planned)
    );
    Ok((rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Failed, unread};

    #[test]
    fn only_a_git_that_ran_and_failed_is_told_to_fetch_the_commits() {
        let failed = |why: &str, started: bool, timed_out: bool| Failed {
            why: why.to_owned(),
            started,
            timed_out,
        };
        let hint = "fetch abc and main into the mounted repository";
        for (given, hinted) in [
            (
                failed("git cannot be run: gone; put git on PATH", false, false),
                false,
            ),
            (
                failed("`git diff` timed out after 20 s in /r", true, true),
                false,
            ),
            (
                failed("`git diff` failed in /r: bad object", true, false),
                true,
            ),
        ] {
            let why = given.why.clone();
            let said = unread(given, "abc", "main");
            assert!(said.starts_with(&why), "{said}");
            assert_eq!(said.contains(hint), hinted, "{said}");
        }
    }
}
