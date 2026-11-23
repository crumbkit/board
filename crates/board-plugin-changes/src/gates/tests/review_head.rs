//! Decision 1318Z: a review stays valid through later commits that edit
//! only record paths or cleanly merge main. These tests walk real git
//! repositories through `diff::after` and judge the commits with
//! `diff::accepted`; a git that times out is a host that says so.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use board_ops::{Host, Limit, QueryError, QueryReport, Run, Settings, Variables};
use board_plugin::child::{Exited, ProcessError};

use crate::diff::{Mounted, Step, accepted, after};

/// A host that only runs processes, with the test's `PATH`; with `stalled`,
/// every process times out.
struct Git {
    settings: Settings,
    stalled: bool,
}

impl Git {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture's git must be found on the test's own PATH"
    )]
    fn new(stalled: bool) -> Self {
        let path = std::env::var_os("PATH").unwrap_or_default();
        Self {
            settings: Settings {
                now: std::time::SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables(vec![("PATH".to_owned(), path)]),
                cache_home: None,
            },
            stalled,
        }
    }
}

impl Host for Git {
    fn query(&self, _sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        unreachable!("the walk reads no board")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        unreachable!("the walk resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        unreachable!("the walk reads no files")
    }

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        unreachable!("the walk writes nothing")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        unreachable!("the walk calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        unreachable!("the walk opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        unreachable!("the walk asks the owner nothing")
    }

    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("the walk syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("the walk updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }

    fn run(&self, run: &Run<'_>) -> Result<Exited, ProcessError> {
        if self.stalled {
            return Err(ProcessError::Timeout { stderr: Vec::new() });
        }
        self.run_hiding(run, &[])
    }
}

/// A git repository in a directory of its own under the system's temporary
/// directory, removed when dropped.
struct Repo {
    root: PathBuf,
    host: Git,
}

impl Drop for Repo {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture removes the repository it made"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Repo {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture makes a git repository as a builder would"
    )]
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("board-review-head-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = Self {
            root,
            host: Git::new(false),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("src/a.rs", "one\n", "start");
        repo
    }

    /// What `git <arguments>` prints, trimmed, with an identity of its own.
    fn git(&self, arguments: &[&str]) -> String {
        let mut owned: Vec<String> = [
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ]
        .iter()
        .map(|each| (*each).to_owned())
        .collect();
        owned.extend(arguments.iter().map(|each| (*each).to_owned()));
        let run = Run {
            program: "git",
            arguments: &owned,
            directory: Some(&self.root),
            input: b"",
            deadline: Duration::from_secs(20),
            added: &[
                ("GIT_CONFIG_GLOBAL", "/dev/null"),
                ("GIT_CONFIG_NOSYSTEM", "1"),
            ],
        };
        let exited = self.host.run(&run).unwrap();
        String::from_utf8_lossy(&exited.stdout).trim().to_owned()
    }

    /// Writes `text` at `file` and commits it with `message`; returns the
    /// commit.
    #[expect(
        clippy::disallowed_methods,
        reason = "the fixture edits its repository as a builder would"
    )]
    fn commit(&self, file: &str, text: &str, message: &str) -> String {
        let at = self.root.join(file);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, text).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// The repository mounted at /repos/x, showing the directory `prefix`
    /// names, or the whole repository when it is empty.
    fn mounted(&self, prefix: &str) -> Mounted {
        Mounted {
            repository: self.root.clone(),
            follow: "main".to_owned(),
            prefix: prefix.to_owned(),
            mount: "/repos/x".to_owned(),
        }
    }

    /// The commits after `from` to `to`, judged with record paths under
    /// /repos/x/plan/.
    fn judged(&self, from: &str, to: &str) -> Result<Vec<String>, String> {
        self.judged_in("", from, to)
    }

    /// [`Repo::judged`] with the mount showing the directory `prefix` names.
    fn judged_in(&self, prefix: &str, from: &str, to: &str) -> Result<Vec<String>, String> {
        let steps = after(&self.host, &self.mounted(prefix), from, to)?;
        accepted(&steps, &allowed(), "/repos/x")
    }
}

/// The files a record-paths rule lists in these fixtures.
fn allowed() -> BTreeSet<String> {
    ["/repos/x/plan/notes.md".to_owned()].into_iter().collect()
}

/// A repository with `work` branched from main and a reviewed commit on it.
fn reviewed(name: &str) -> (Repo, String) {
    let repo = Repo::new(name);
    repo.git(&["checkout", "-q", "-b", "work"]);
    let head = repo.commit("src/b.rs", "built\n", "build");
    (repo, head)
}

#[test]
fn a_commit_that_edits_only_a_record_path_keeps_the_review_and_a_code_commit_does_not() {
    let (repo, reviewed) = reviewed("record");
    let record = repo.commit("plan/notes.md", "noted\n", "note");
    assert_eq!(repo.judged(&reviewed, &record), Ok(vec![record.clone()]));
    let code = repo.commit("src/c.rs", "more\n", "code");
    let refused = repo.judged(&reviewed, &code).unwrap_err();
    assert!(
        refused.starts_with(&format!(
            "the later commit {code} edits /repos/x/src/c.rs, which is not a record path"
        )),
        "{refused}"
    );
}

#[test]
fn a_commit_that_edits_a_file_outside_the_mounts_directory_loses_the_review() {
    let (repo, reviewed) = reviewed("outside");
    let record = repo.commit("sub/plan/notes.md", "noted\n", "note");
    assert_eq!(
        repo.judged_in("sub/", &reviewed, &record),
        Ok(vec![record.clone()])
    );
    let code = repo.commit("src/c.rs", "more\n", "code");
    let refused = repo.judged_in("sub/", &reviewed, &code).unwrap_err();
    assert!(
        refused.starts_with(&format!(
            "the later commit {code} edits src/c.rs, outside the directory /repos/x shows"
        )),
        "{refused}"
    );
}

#[test]
fn a_clean_merge_of_main_keeps_the_review_and_a_conflicted_one_does_not() {
    let (repo, reviewed_head) = reviewed("merge");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("src/m.rs", "main\n", "main moves");
    repo.git(&["checkout", "-q", "work"]);
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    let merged = repo.git(&["rev-parse", "HEAD"]);
    let steps = after(&repo.host, &repo.mounted(""), &reviewed_head, &merged).unwrap();
    assert_eq!(
        steps,
        [Step::Merge {
            commit: merged.clone(),
            clean: true
        }]
    );
    assert_eq!(repo.judged(&reviewed_head, &merged), Ok(vec![merged]));

    let repo = Repo::new("conflict");
    repo.git(&["checkout", "-q", "-b", "work"]);
    let reviewed_head = repo.commit("src/a.rs", "work\n", "work edits a");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("src/a.rs", "main\n", "main edits a");
    repo.git(&["checkout", "-q", "work"]);
    // The merge stops on the conflict, and the builder settles it.
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    let merged = repo.commit("src/a.rs", "settled\n", "merge main");
    assert_eq!(
        repo.judged(&reviewed_head, &merged),
        Err(format!(
            "the later commit {merged} is a merge that is not a clean merge of main; review the \
             head"
        ))
    );
}

#[test]
fn a_merge_whose_tree_differs_from_what_git_merges_is_not_clean() {
    let (repo, reviewed_head) = reviewed("amended");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("src/m.rs", "main\n", "main moves");
    repo.git(&["checkout", "-q", "work"]);
    repo.git(&["merge", "-q", "--no-commit", "main"]);
    repo.commit("src/hidden.rs", "slipped in\n", "merge main");
    let merged = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"])
            .split(' ')
            .count(),
        3
    );
    let refused = repo.judged(&reviewed_head, &merged).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "the later commit {merged} is a merge that is not a clean merge of main; review the \
             head"
        )
    );
}

#[test]
fn a_rebase_after_review_loses_the_review_and_says_to_merge_main() {
    let (repo, reviewed_head) = reviewed("rebase");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("src/m.rs", "main\n", "main moves");
    repo.git(&["checkout", "-q", "work"]);
    repo.git(&["rebase", "-q", "main"]);
    let rebased = repo.git(&["rev-parse", "HEAD"]);
    let refused = repo.judged(&reviewed_head, &rebased).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "{reviewed_head} is not on the first-parent chain of {rebased}, as after a rebase; \
             merge main in place of rebasing, or review {rebased}"
        )
    );
}

#[test]
fn a_git_that_times_out_loses_the_review_and_says_why() {
    let repo = Repo::new("stalled");
    let stalled = Git::new(true);
    let head = "0123456789abcdef0123456789abcdef01234567";
    let refused = after(&stalled, &repo.mounted(""), head, head).unwrap_err();
    assert!(refused.contains("timed out after 20 s"), "{refused}");
}

/// The review gate counts a report on an earlier commit when observe found
/// the commits after it accepted, names them in the pass detail, and says
/// why when they are not.
#[test]
fn the_review_check_passes_a_report_whose_later_commits_were_accepted_and_names_them() {
    use serde_json::json;
    let later = "b3f9c0de1234567890abcdef1234567890abcdef";
    let reviewed = super::HEAD;
    let change = super::given("review")
        .records(vec![(
            "/r/1",
            super::report(&json!({"verdict": "approve"})),
        )])
        .with(json!({"head": later}));
    let mut observed = change.observed();
    let check = |observed: &crate::gates::Observed| {
        let found = crate::gates::gate(observed);
        let review = found
            .into_iter()
            .find(|each| each.check == "review")
            .unwrap();
        (review.status, review.detail)
    };
    let (status, detail) = check(&observed);
    assert_eq!(status, crate::gates::Status::Missing, "{detail}");
    observed
        .after_review
        .insert(reviewed.to_owned(), Ok(vec![later.to_owned()]));
    let (status, detail) = check(&observed);
    assert_eq!(status, crate::gates::Status::Pass);
    assert_eq!(
        detail,
        format!(
            "/r/1 covers an earlier commit; the commits after it, {later}, edit only record \
             paths or cleanly merge main, so it still counts"
        )
    );
    let why = "the later commit c edits /repos/x/src/c.rs, which is not a record path; review \
               the head";
    observed
        .after_review
        .insert(reviewed.to_owned(), Err(why.to_owned()));
    let (status, detail) = check(&observed);
    assert_eq!(status, crate::gates::Status::Missing);
    assert!(
        detail.ends_with(&format!("; /r/1 covers an earlier commit, and {why}")),
        "{detail}"
    );
}
