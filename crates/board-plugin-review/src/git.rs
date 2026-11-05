//! The commits under review, read with git from the repository that holds
//! the board or a mount's clone. The plugin reads commits and never writes
//! to the repository: review records reach the board only through
//! `Host::put`. Git runs without the user's or the system's config, and
//! diffs take fixed options, so a patch id is the same on every machine.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use board_plugin::child::{Exited, ProcessError};
use board_plugin::shape::Record;
use serde_json::{Map, Value, json};

use crate::records::{BriefRecord, ReportRecord};
use crate::refusal;

/// How long one git command may run.
const TIMEOUT: Duration = Duration::from_mins(1);
/// The variables that keep the user's and the system's git config out of a
/// git command, and stop it prompting.
const ISOLATED: [(&str, &str); 3] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
];
/// The model credentials `run` passes the agent, which git never needs.
const HIDDEN: [&str; 2] = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];
/// Options that fix a diff's text, whatever the repository's own config
/// says. `patch_ids` pastes these and `PATCH` into a shell script unquoted,
/// so each must stay shell-safe: letters, digits and `-=./_` only.
const CONFIG: [&str; 4] = ["-c", "diff.algorithm=myers", "-c", "diff.noprefix=false"];
const DIFF: [&str; 8] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--text",
    "-U3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];
/// The options plan/review.md gives a patch id's diff, with those a
/// repository's config could change fixed. Like `CONFIG`, they must stay
/// shell-safe.
const PATCH: [&str; 9] = [
    "-p",
    "--full-index",
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "-U3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];
/// The exit code of `patch_ids`'s script when `git diff-tree` failed.
const DIFF_TREE_FAILED: i32 = 2;
/// The exit code of `patch_ids`'s script when `git patch-id` failed.
const PATCH_ID_FAILED: i32 = 3;
/// The directory, from the board's root, that holds every review record.
const REVIEW_PATH: &str = "reviews/";
/// How much of git's stderr a failure carries.
const TAIL: usize = 4096;

/// The git repository whose working tree is at `root`.
pub(crate) struct Repository<'a> {
    pub(crate) host: &'a dyn Host,
    pub(crate) root: PathBuf,
}

impl Repository<'_> {
    /// What `git <arguments>` printed, or `review::git::failed`.
    fn bytes(&self, arguments: &[&str], stdin: &[u8]) -> Result<Vec<u8>, PluginError> {
        let command = arguments.first().copied().unwrap_or("git");
        let arguments: Vec<String> = (CONFIG.iter().chain(arguments))
            .map(|argument| (*argument).to_owned())
            .collect();
        let exited = self.exited("git", &arguments, command, stdin)?;
        if exited.code == Some(0) {
            return Ok(exited.stdout);
        }
        Err(self.failure(command, &exited))
    }

    /// How `program <arguments>` exited, or `review::git::failed` naming
    /// `git <command>` when it did not start, ran too long or printed too
    /// much.
    fn exited(
        &self,
        program: &str,
        arguments: &[String],
        command: &str,
        stdin: &[u8],
    ) -> Result<Exited, PluginError> {
        let run = Run {
            program,
            arguments,
            directory: Some(&self.root),
            input: stdin,
            deadline: TIMEOUT,
            added: &ISOLATED,
        };
        let root = self.root.display();
        match self.host.run_hiding(&run, &HIDDEN) {
            Ok(exited) => Ok(exited),
            Err(ProcessError::NotStarted { .. }) if !self.root.is_dir() => Err(failed(
                format!(
                    "the repository's directory {root} does not exist, so git cannot run \
                     there; check the path, or the clone a mount reads"
                ),
                Map::new(),
            )),
            Err(ProcessError::NotStarted { source }) => Err(failed(
                format!("{program} cannot be run: {source}; put {program} on PATH"),
                Map::new(),
            )),
            Err(ProcessError::Timeout { .. }) => Err(failed(
                format!(
                    "`git {command}` timed out after {} s in {root}; run the review again \
                     once git is not held up",
                    TIMEOUT.as_secs()
                ),
                Map::new(),
            )),
            Err(ProcessError::TooLarge { limit, .. }) => Err(failed(
                format!(
                    "`git {command}` printed more than {limit} bytes in {root}; review a \
                     shorter range"
                ),
                Map::new(),
            )),
        }
    }

    /// `review::git::failed` for `git <command>`, which exited as `exited`
    /// says, with the end of its stderr.
    fn failure(&self, command: &str, exited: &Exited) -> PluginError {
        let stderr = String::from_utf8_lossy(&exited.stderr);
        let stderr = board_ops::agent::tail(stderr.trim(), TAIL).to_owned();
        let mut fields = Map::new();
        fields.insert("stderr".to_owned(), json!(stderr));
        failed(
            format!(
                "`git {command}` failed in {}: {stderr}; check that the board's root is in a \
                 git repository that holds the commits, and fetch them if it does not",
                self.root.display()
            ),
            fields,
        )
    }

    fn git(&self, arguments: &[&str]) -> Result<String, PluginError> {
        let bytes = self.bytes(arguments, b"")?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The full id of the commit `revision` names, or
    /// `review::range::unknown`.
    pub(crate) fn commit(&self, revision: &str) -> Result<String, PluginError> {
        let named = format!("{revision}^{{commit}}");
        let found = self.git(&["rev-parse", "--verify", "--end-of-options", &named]);
        found.map(|id| id.trim().to_owned()).map_err(|_| {
            refusal(
                "review::range::unknown",
                format!(
                    "'{revision}' names no commit in {}; fetch it, or name a commit `git log` \
                     shows",
                    self.root.display()
                ),
                [("revision", json!(revision))],
            )
        })
    }

    /// Every commit from `base` to `head`, merges among them, oldest first.
    pub(crate) fn commits(&self, base: &str, head: &str) -> Result<Vec<String>, PluginError> {
        let range = format!("{base}..{head}");
        let listed = self.git(&["rev-list", "--reverse", "--end-of-options", &range])?;
        Ok(listed.split_whitespace().map(str::to_owned).collect())
    }

    /// Every commit in the range `commits`, or none when git cannot resolve
    /// it, as for a commit not fetched.
    pub(crate) fn within(&self, commits: &str) -> HashSet<String> {
        let listed = self.git(&["rev-list", "--end-of-options", commits]);
        (listed.unwrap_or_default().split_whitespace())
            .map(str::to_owned)
            .collect()
    }

    /// The patch id of each of `commits` that has one, by commit: `git
    /// diff-tree -p --full-index <commit> | git patch-id --verbatim`. A
    /// merge or an empty commit prints no patch, so it has no patch id, and
    /// `--verbatim` keeps whitespace, so a change of indentation alone
    /// changes the id. For a binary file git hashes the full blob ids, so
    /// two binary changes to one path differ.
    pub(crate) fn patch_ids(
        &self,
        commits: &[String],
    ) -> Result<HashMap<String, String>, PluginError> {
        if commits.is_empty() {
            return Ok(HashMap::new());
        }
        let stdin = commits.join("\n") + "\n";
        // A commit's patch can be larger than Board keeps of a process's
        // output, so git pipes it to patch-id and only the ids come back. A
        // pipeline's status is its last command's, so diff-tree echoes its
        // own status to fd 4, which the command substitution captures, while
        // patch-id writes the ids to the script's stdout through fd 3. git
        // may print hints on stderr and still succeed, so only the two exit
        // codes decide whether it failed.
        let config = CONFIG.join(" ");
        let script = format!(
            "exec 3>&1\n\
             status=$( {{ {{ git {config} diff-tree --stdin {}; echo $? >&4; }} \
             | git {config} patch-id --verbatim >&3; }} 4>&1 ) || exit {PATCH_ID_FAILED}\n\
             [ \"$status\" = 0 ] || exit {DIFF_TREE_FAILED}\n",
            PATCH.join(" ")
        );
        let arguments = ["-c".to_owned(), script];
        let pipeline = "diff-tree | git patch-id";
        let exited = self.exited("sh", &arguments, pipeline, stdin.as_bytes())?;
        let listed = match exited.code {
            Some(0) => exited.stdout,
            Some(DIFF_TREE_FAILED) => return Err(self.failure("diff-tree", &exited)),
            Some(PATCH_ID_FAILED) => return Err(self.failure("patch-id", &exited)),
            _ => return Err(self.failure(pipeline, &exited)),
        };
        Ok((String::from_utf8_lossy(&listed).lines())
            .filter_map(|line| {
                let (patch_id, commit) = line.split_once(' ')?;
                Some((commit.trim().to_owned(), patch_id.to_owned()))
            })
            .collect())
    }

    /// The blob id at HEAD of each file under /reviews/, by its path from
    /// the board's root.
    pub(crate) fn committed(&self) -> Result<HashMap<String, String>, PluginError> {
        let listed = self.git(&["ls-tree", "-r", "-z", "HEAD", "--", REVIEW_PATH])?;
        Ok((listed.split('\0'))
            .filter_map(|entry| {
                let (line, name) = entry.split_once('\t')?;
                let blob = line.split_whitespace().nth(2)?;
                Some((name.to_owned(), blob.to_owned()))
            })
            .collect())
    }

    /// The best common ancestor of `base` and `head`, or
    /// `review::range::unrelated` when they have none.
    pub(crate) fn merge_base(&self, base: &str, head: &str) -> Result<String, PluginError> {
        let found = self.git(&["merge-base", "--end-of-options", base, head]);
        found.map(|id| id.trim().to_owned()).map_err(|_| {
            refusal(
                "review::range::unrelated",
                format!(
                    "{base} and {head} share no commit; name the pull request's base commit as \
                     --base, and in a shallow clone fetch the full history first, as with `git \
                     fetch --unshallow` or `fetch-depth: 0`"
                ),
                [("base", json!(base)), ("head", json!(head))],
            )
        })
    }

    /// The diff from `base` to `head`.
    pub(crate) fn diff(&self, base: &str, head: &str) -> Result<String, PluginError> {
        let arguments: Vec<&str> = (["diff"].into_iter().chain(DIFF))
            .chain(["--end-of-options", base, head])
            .collect();
        self.git(&arguments)
    }

    /// Whether `commit` only adds briefs and reports at their paths under
    /// /reviews/, or sets a report's `source`, which `review.post` sets to
    /// the comment's URL and which changes nothing the report covers.
    pub(crate) fn review_only(&self, commit: &str) -> Result<bool, PluginError> {
        let listed = |filter: &[&str]| -> Result<Vec<String>, PluginError> {
            let mut arguments = vec![
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--no-renames",
                "--name-only",
                "-r",
                "-z",
            ];
            arguments.extend(filter);
            arguments.extend(["--end-of-options", commit]);
            let printed = self.git(&arguments)?;
            Ok((printed.split('\0'))
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect())
        };
        let changed = listed(&[])?;
        let added = listed(&["--diff-filter=A"])?;
        Ok(!changed.is_empty()
            && changed.iter().all(|path| {
                self.record(commit, path) && (added.contains(path) || self.sourced(commit, path))
            }))
    }

    /// The JSON object `commit` holds at `path`, if it holds one.
    fn object(&self, commit: &str, path: &str) -> Option<Map<String, Value>> {
        let text = self
            .git(&["cat-file", "blob", &format!("{commit}:{path}")])
            .ok()?;
        match serde_json::from_str(&text) {
            Ok(Value::Object(object)) => Some(object),
            _ => None,
        }
    }

    /// Whether `commit` changes only the `source` of the report at `path`.
    fn sourced(&self, commit: &str, path: &str) -> bool {
        if !path.ends_with("/report.json") {
            return false;
        }
        let (Some(mut before), Some(mut after)) = (
            self.object(&format!("{commit}^"), path),
            self.object(commit, path),
        ) else {
            return false;
        };
        before.remove("source");
        after.remove("source");
        before == after
    }

    /// Whether `path` in `commit` is a brief or a report at `/reviews/<id>/`
    /// of the type its name gives.
    fn record(&self, commit: &str, path: &str) -> bool {
        let parts: Vec<&str> = (path.strip_prefix(REVIEW_PATH))
            .map(|rest| rest.split('/').collect())
            .unwrap_or_default();
        let record_type = match parts[..] {
            [id, "brief.json"] if !id.is_empty() => BriefRecord::TYPE,
            [id, "report.json"] if !id.is_empty() => ReportRecord::TYPE,
            _ => return false,
        };
        (self.object(commit, path))
            .is_some_and(|record| record.get("$type") == Some(&json!(record_type)))
    }
}

/// `review::git::failed`, saying `why`.
fn failed(why: String, fields: Map<String, Value>) -> PluginError {
    PluginError {
        code: "review::git::failed".to_owned(),
        message: why,
        fields,
        hint: None,
    }
}

/// The repository whose working tree is at `root`, read through `host`.
pub(crate) fn at<'a>(host: &'a dyn Host, root: &Path) -> Repository<'a> {
    Repository {
        host,
        root: root.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::{CONFIG, PATCH};

    #[test]
    fn the_options_pasted_into_the_patch_id_script_are_shell_safe() {
        for option in CONFIG.iter().chain(&PATCH) {
            assert!(
                (option.chars()).all(|c| c.is_ascii_alphanumeric() || "-=./_".contains(c)),
                "{option}"
            );
        }
    }
}
