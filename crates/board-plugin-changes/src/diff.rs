//! Running git in a mounted repository, the files a change edits, and those
//! no planned path matches.
//!
//! A change's diff runs from where its head left the branch the mount
//! follows to its head, so it holds every commit of the change and none the
//! branch gained since. git runs without the user's or the system's config,
//! so an alias, an external diff driver or a rename setting cannot change
//! which files it lists.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, Run, literal};
use board_plugin::child::{Exited, ProcessError};
use serde_json::Value;

use crate::board::{self, rows, text};
use crate::gates::same_commit;

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
    exited(host, repository, arguments, deadline, &[0]).map(|(_, stdout)| stdout)
}

/// The exit code and output of `git <arguments>` in `repository`, run as
/// [`git`] runs it, when the code is one of `codes`, or why it failed.
fn exited(
    host: &dyn Host,
    repository: &Path,
    arguments: &[&str],
    deadline: Duration,
    codes: &[i32],
) -> Result<(i32, String), Failed> {
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
        Ok(Exited {
            code: Some(code),
            stdout,
            ..
        }) if codes.contains(&code) => Ok((code, String::from_utf8_lossy(&stdout).into_owned())),
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
    let at = match mounted(host, mount)? {
        Ok(at) => at,
        Err(why) => return Ok(Err(why)),
    };
    let arguments = [
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        "--merge-base",
        "--end-of-options",
        &at.follow,
        head,
    ];
    let names = match git(host, &at.repository, &arguments, DIFF_DEADLINE) {
        Ok(names) => names,
        Err(failed) => return Ok(Err(unread(failed, head, &at.follow))),
    };
    Ok(Ok(at.logical(&names)))
}

/// A mounted repository: where it is, the branch the mount follows, the
/// directory of the repository the mount shows, and the mount's path.
#[derive(Debug)]
pub(crate) struct Mounted {
    pub(crate) repository: PathBuf,
    pub(crate) follow: String,
    pub(crate) prefix: String,
    pub(crate) mount: String,
}

impl Mounted {
    /// The logical path of each file `names` lists, NUL-separated, under
    /// the mount; a file outside the directory the mount shows is left out.
    fn logical(&self, names: &str) -> Vec<String> {
        self.split(names).0
    }

    /// The files `names` lists, NUL-separated: by logical path those in the
    /// directory the mount shows, and by repository path those outside it.
    fn split(&self, names: &str) -> (Vec<String>, Vec<String>) {
        let mut inside = Vec::new();
        let mut outside = Vec::new();
        for name in names.split('\0').filter(|name| !name.is_empty()) {
            match name.strip_prefix(&self.prefix) {
                Some(under) => inside.push(format!("{}/{under}", self.mount)),
                None => outside.push(name.to_owned()),
            }
        }
        (inside, outside)
    }
}

/// The repository `mount` resolves to, or why it names none.
pub(crate) fn mounted(
    host: &dyn Host,
    mount: &str,
) -> Result<Result<Mounted, String>, PluginError> {
    let Some(answer) = board::resolve(host, mount)? else {
        return Ok(Err(format!(
            "nothing is at {mount}, so the change's diff cannot be read; check the mount with \
             `board ls /repos`"
        )));
    };
    let location = answer.location;
    let Some(follow) = (location.pointers.last()).and_then(|pointer| pointer.follow.clone()) else {
        return Ok(Err(format!(
            "{mount} is not a mount, so it follows no branch to diff from; set the change's mount \
             to a mount"
        )));
    };
    let prefix = if location.path.is_empty() {
        String::new()
    } else {
        format!("{}/", location.path)
    };
    Ok(Ok(Mounted {
        repository: location.board.repository,
        follow,
        prefix,
        mount: mount.trim_end_matches('/').to_owned(),
    }))
}

/// One commit after a reviewed commit: the files it edits, by logical
/// path, with those outside the directory the mount shows by repository
/// path, or a merge and whether it cleanly merges the branch the mount
/// follows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Edit {
        commit: String,
        files: Vec<String>,
        outside: Vec<String>,
    },
    Merge {
        commit: String,
        clean: bool,
    },
}

/// The commits on the first-parent chain after `from` up to `to`, oldest
/// first, or why they cannot be read. A `from` that is not on that chain,
/// as after a rebase, has none.
pub(crate) fn after(
    host: &dyn Host,
    at: &Mounted,
    from: &str,
    to: &str,
) -> Result<Vec<Step>, String> {
    let repository = at.repository.as_path();
    let run = |arguments: &[&str]| git(host, repository, arguments, DIFF_DEADLINE);
    let failed = |failed: Failed| failed.why;
    let commit = |name: &str| {
        run(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{name}^{{commit}}"),
        ])
        .map(|sha| sha.trim().to_owned())
        .map_err(|failed| format!("{}; fetch {name} into the mounted repository", failed.why))
    };
    let (from, to) = (commit(from)?, commit(to)?);
    let rebased = || {
        format!(
            "{from} is not on the first-parent chain of {to}, as after a rebase; merge main in \
             place of rebasing, or review {to}"
        )
    };
    let (code, _) = exited(
        host,
        repository,
        &[
            "merge-base",
            "--is-ancestor",
            "--end-of-options",
            &from,
            &to,
        ],
        DIFF_DEADLINE,
        &[0, 1],
    )
    .map_err(failed)?;
    if code != 0 {
        return Err(rebased());
    }
    let range = format!("{from}..{to}");
    let listed = run(&[
        "rev-list",
        "--first-parent",
        "--parents",
        "--end-of-options",
        &range,
    ])
    .map_err(failed)?;
    let mut chain: Vec<Vec<&str>> = (listed.lines())
        .map(|line| line.split_whitespace().collect())
        .filter(|line: &Vec<&str>| !line.is_empty())
        .collect();
    chain.reverse();
    let first_parent = chain.first().and_then(|line| line.get(1).copied());
    if !chain.is_empty() && first_parent != Some(from.as_str()) {
        return Err(rebased());
    }
    let mut steps = Vec::new();
    for line in chain {
        let (commit, parents) = (line[0], &line[1..]);
        steps.push(match parents {
            [parent] => {
                let names = run(&[
                    "diff-tree",
                    "-r",
                    "--no-commit-id",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    "--end-of-options",
                    parent,
                    commit,
                ])
                .map_err(failed)?;
                let (files, outside) = at.split(&names);
                Step::Edit {
                    commit: commit.to_owned(),
                    files,
                    outside,
                }
            }
            [ours, theirs] => Step::Merge {
                commit: commit.to_owned(),
                clean: clean_merge(host, at, commit, ours, theirs)?,
            },
            _ => Step::Merge {
                commit: commit.to_owned(),
                clean: false,
            },
        });
    }
    Ok(steps)
}

/// Whether the merge `commit` of `ours` and `theirs` is a clean merge of
/// the branch the mount follows: `theirs` is on that branch, and the
/// commit's tree is the one git merges without conflict.
fn clean_merge(
    host: &dyn Host,
    at: &Mounted,
    commit: &str,
    ours: &str,
    theirs: &str,
) -> Result<bool, String> {
    let repository = at.repository.as_path();
    let ancestor = [
        "merge-base",
        "--is-ancestor",
        "--end-of-options",
        theirs,
        &at.follow,
    ];
    let (code, _) =
        exited(host, repository, &ancestor, DIFF_DEADLINE, &[0, 1]).map_err(|failed| failed.why)?;
    if code != 0 {
        return Ok(false);
    }
    let merged = [
        "merge-tree",
        "--write-tree",
        "--end-of-options",
        ours,
        theirs,
    ];
    let (code, tree) =
        exited(host, repository, &merged, DIFF_DEADLINE, &[0, 1]).map_err(|failed| failed.why)?;
    if code != 0 {
        return Ok(false);
    }
    let tree_of = format!("{commit}^{{tree}}");
    let written = git(
        host,
        repository,
        &["rev-parse", "--verify", "--end-of-options", &tree_of],
        DIFF_DEADLINE,
    )
    .map_err(|failed| failed.why)?;
    Ok(tree.lines().next().map(str::trim) == Some(written.trim()))
}

/// Whether git reports a conflict when it merges `head` into the branch
/// the mount follows, read with `git merge-tree --write-tree` within
/// `deadline`, or why git could not say.
pub(crate) fn conflicts(
    host: &dyn Host,
    at: &Mounted,
    head: &str,
    deadline: Duration,
) -> Result<bool, String> {
    let merged = [
        "merge-tree",
        "--write-tree",
        "--end-of-options",
        &at.follow,
        head,
    ];
    let (code, out) =
        exited(host, &at.repository, &merged, deadline, &[0, 1]).map_err(|failed| failed.why)?;
    // git also exits 1 when it cannot merge a commit at all; only a conflict
    // writes the merged tree's id first.
    let tree = out.lines().next().map_or_default(str::trim);
    let written = !tree.is_empty() && tree.bytes().all(|byte| byte.is_ascii_hexdigit());
    match (code, written) {
        (1, true) => Ok(true),
        (1, false) => Err(format!(
            "git merge-tree could not merge {head} into {}",
            at.follow
        )),
        _ => Ok(false),
    }
}

/// The commits of `steps`, when each edits only files in `allowed`, is a
/// clean merge or is one of the `fixed` commits, whole or abbreviated, or
/// else why the first that does not fails. A commit that edits a file
/// outside the directory the mount shows fails, even a fixed one.
pub(crate) fn accepted(
    steps: &[Step],
    allowed: &BTreeSet<String>,
    fixed: &[String],
    mount: &str,
) -> Result<Vec<String>, String> {
    let mut commits = Vec::new();
    for step in steps {
        match step {
            Step::Edit {
                commit,
                files,
                outside,
            } => {
                if !outside.is_empty() {
                    return Err(format!(
                        "the later commit {commit} edits {}, outside the directory {mount} \
                         shows, which is not a record path; review the head",
                        outside.join(", ")
                    ));
                }
                if (fixed.iter()).any(|each| same_commit(Some(&Value::from(each.as_str())), commit))
                {
                    commits.push(commit.clone());
                    continue;
                }
                let other: Vec<&str> = (files.iter())
                    .filter(|file| !allowed.contains(*file))
                    .map(String::as_str)
                    .collect();
                if !other.is_empty() {
                    return Err(format!(
                        "the later commit {commit} edits {}, which is neither a record path nor \
                         the lifecycle doc, and no changes/fix/v1 names it; review the head",
                        other.join(", ")
                    ));
                }
                commits.push(commit.clone());
            }
            Step::Merge { commit, clean } => {
                if !clean {
                    return Err(format!(
                        "the later commit {commit} is a merge that is not a clean merge of main; \
                         review the head"
                    ));
                }
                commits.push(commit.clone());
            }
        }
    }
    Ok(commits)
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
/// rules match their paths.
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
