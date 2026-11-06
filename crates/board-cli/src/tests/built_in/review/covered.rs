//! `review.covered` passes when committed reports cover each commit of a
//! pull request, by range or by patch id, and with `--change` when the
//! reports a change attaches cover the commits of the clone its mount reads.

use super::*;

/// The changes plugin's change and plan schemas, which the board holds so a
/// change and the plan that attaches its reports read as valid without
/// enabling the changes plugin.
const CHANGE_SCHEMA: &str =
    include_str!("../../../../../board-plugin-changes/types/changes/change/v2.schema.json");
const PLAN_SCHEMA: &str =
    include_str!("../../../../../board-plugin-changes/types/changes/plan/v1.schema.json");

/// Two commits on a branch, reviewed: the base, the head and the report,
/// which `board put` commits.
fn reviewed(repo: &Repo) -> (String, String, String) {
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    repo.commit("src/a.txt", "one\n");
    let head = repo.commit("src/b.txt", "two\n");
    let brief = repo.brief("change", &format!("{base}..{head}"), &json!({}));
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
    (base, head, output["report"].as_str().unwrap().to_owned())
}

/// Commits a person's report at /reviews/<name>/report of `base..head`,
/// approving unless `fields` differ, and returns its path.
fn report(repo: &Repo, name: &str, base: &str, head: &str, fields: &Value) -> String {
    let record = record(
        json!({
            "$type": "review/report/v1",
            "range": format!("{base}..{head}"),
            "patch_ids": [],
            "findings": [],
            "verdict": "approve",
            "by": {"person": "a-person"},
        }),
        fields,
    );
    repo.commit(&format!("reviews/{name}/report.json"), &record);
    format!("/reviews/{name}/report")
}

/// The fields of a `review::head::uncovered` refusal of `covered --base
/// main <flags>`.
fn uncovered(repo: &Repo, flags: &[&str]) -> Value {
    let flags: Vec<&str> = ["--base", "main"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let (code, document) = repo.call("covered", &flags);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::head::uncovered")),
        "{document}"
    );
    document["fields"].clone()
}

fn covered(repo: &Repo, flags: &[&str]) -> Value {
    let flags: Vec<&str> = ["--base", "main"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let (code, output) = repo.call("covered", &flags);
    assert_eq!(code, 0, "{output}");
    output
}

/// The patch id of each of `commits` that has one, as `review.run` records
/// them.
fn patch_ids(repo: &Repo, name: &str, base: &str, head: &str) -> Value {
    let brief = repo.brief(name, &format!("{base}..{head}"), &json!({}));
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
    repo.get(&format!("/reviews/{name}/report"))["patch_ids"].clone()
}

#[test]
fn a_report_whose_range_ends_at_the_head_covers_it_past_the_commits_that_record_it() {
    let repo = Repo::new();
    let (base, head, path) = reviewed(&repo);
    let tip = repo.git(&["rev-parse", "HEAD"]);
    // The brief's commit and the report's commit only add review records.
    assert_eq!(
        covered(&repo, &[]),
        json!({"head": tip, "merge_base": base, "skipped": 2, "commits": 2, "reports": [path]})
    );
    let output = covered(&repo, &["--head", &head]);
    assert_eq!(
        (&output["skipped"], &output["reports"]),
        (&json!(0), &json!([path]))
    );
}

#[test]
fn a_head_with_a_change_after_the_report_is_refused_and_the_reviewed_head_admitted() {
    let repo = Repo::new();
    let (_, head, _) = reviewed(&repo);
    let later = repo.commit("src/c.txt", "three\n");
    let fields = uncovered(&repo, &[]);
    assert_eq!(
        (
            &fields["head"],
            &fields["skipped"],
            &fields["reports"],
            &fields["uncovered"]
        ),
        (&json!(later), &json!(2), &json!(1), &json!([later]))
    );
    covered(&repo, &["--head", &head]);
}

/// Branches `name` from main, remakes the reviewed commits with
/// `src/b.txt` as `b`, and copies the review commits.
fn replayed(repo: &Repo, head: &str, name: &str, b: &str) {
    let records = repo.git(&["rev-list", "--reverse", &format!("{head}..change")]);
    repo.git(&["checkout", "-q", "-b", name, "main"]);
    repo.commit("src/a.txt", "one\n");
    repo.commit("src/b.txt", b);
    let mut pick = vec!["cherry-pick"];
    pick.extend(records.split_whitespace());
    repo.git(&pick);
}

#[test]
fn a_rebased_branch_is_covered_by_patch_id_and_a_changed_one_is_not() {
    let repo = Repo::new();
    let (_, head, path) = reviewed(&repo);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("other.txt", "unrelated\n");
    replayed(&repo, &head, "edited", "two, edited\n");
    let fields = uncovered(&repo, &[]);
    // The report lists a patch id the edited branch lacks, so it covers
    // neither commit by patch id.
    assert_eq!(
        (&fields["reports"], &fields["uncovered"]),
        (
            &json!(1),
            &json!([
                repo.git(&["rev-parse", "HEAD~3"]),
                repo.git(&["rev-parse", "HEAD~2"])
            ])
        )
    );
    replayed(&repo, &head, "same", "two\n");
    let output = covered(&repo, &[]);
    assert_eq!(output["reports"], json!([path]));
    assert_ne!(output["head"], json!(head));
}

#[test]
fn a_rebased_commit_whose_indentation_changed_is_refused_and_an_unchanged_one_admitted() {
    let repo = Repo::new();
    let (_, head, _) = reviewed(&repo);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("other.txt", "unrelated\n");
    replayed(&repo, &head, "reindented", "    two\n");
    uncovered(&repo, &[]);
    replayed(&repo, &head, "same", "two\n");
    covered(&repo, &[]);
}

#[test]
fn a_merge_is_refused_by_patch_ids_and_admitted_by_a_range_that_contains_it() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    repo.commit("src/a.txt", "one\n");
    let head = repo.commit("src/b.txt", "two\n");
    let ids = patch_ids(&repo, "ids", &base, &head);
    repo.git(&["reset", "-q", "--hard", &head]);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("other.txt", "unrelated\n");
    repo.git(&["checkout", "-q", "change"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "merge main", "main"]);
    let merge = repo.git(&["rev-parse", "HEAD"]);
    let branch = report(&repo, "branch", &base, &base, &json!({"patch_ids": ids}));
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([merge]));
    let merged = report(&repo, "merged", &head, &merge, &json!({}));
    assert_eq!(covered(&repo, &[])["reports"], json!([branch, merged]));
}

#[test]
fn an_empty_commit_is_refused_by_patch_ids_and_admitted_by_a_range_that_contains_it() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    repo.commit("src/a.txt", "one\n");
    repo.git(&["commit", "-q", "--allow-empty", "-m", "nothing"]);
    let head = repo.commit("src/b.txt", "two\n");
    let brief = repo.brief("change", &format!("{base}..{head}"), &json!({}));
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    let first = output["report"].clone();
    assert_eq!(
        repo.get("/reviews/change/report")["patch_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("other.txt", "unrelated\n");
    repo.git(&["checkout", "-q", "change"]);
    repo.git(&["rebase", "-q", "--empty=keep", "main"]);
    let empty = repo.git(&["rev-list", "-n1", "--grep=nothing", "HEAD"]);
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([empty]));
    report(
        &repo,
        "empty",
        &repo.git(&["rev-parse", &format!("{empty}~1")]),
        &empty,
        &json!({}),
    );
    assert_eq!(
        covered(&repo, &[])["reports"],
        json!([first, "/reviews/empty/report"])
    );
}

#[test]
fn reports_together_cover_a_range_none_covers_alone() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let first = repo.commit("src/a.txt", "one\n");
    let head = repo.commit("src/b.txt", "two\n");
    let tip = report(&repo, "tip", &first, &head, &json!({}));
    assert_eq!(
        uncovered(&repo, &["--head", &head])["uncovered"],
        json!([first])
    );
    let start = report(&repo, "start", &base, &first, &json!({}));
    assert_eq!(
        covered(&repo, &["--head", &head])["reports"],
        json!([start, tip])
    );
}

#[test]
fn a_base_sharing_no_commit_with_the_head_is_refused_and_a_related_one_admitted() {
    let repo = Repo::new();
    reviewed(&repo);
    let empty_tree = repo.git(&["hash-object", "-t", "tree", "/dev/null"]);
    let orphan = repo.git(&["commit-tree", &empty_tree, "-m", "orphan"]);
    let (code, document) = repo.call("covered", &["--base", &orphan]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::range::unrelated"))
    );
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("fetch-depth: 0"), "{message}");
    covered(&repo, &[]);
}

#[test]
fn a_report_reached_through_a_mount_is_ignored_and_the_same_one_in_the_own_tree_admitted() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let head = repo.commit("src/a.txt", "one\n");
    let text = json!({
        "$type": "review/report/v1", "range": format!("{base}..{head}"), "patch_ids": [],
        "findings": [], "verdict": "approve", "by": {"person": "a-person"},
    })
    .to_string();
    let (_library, mount) = library(&[("report.json", &text)]);
    repo.commit("shared.json", &mount.to_string());
    assert_eq!(repo.run(&["get", "/shared/report"], &[]).code, 0);
    assert_eq!(uncovered(&repo, &["--head", &head])["reports"], 0);
    repo.commit("reviews/own/report.json", &text);
    assert_eq!(
        covered(&repo, &["--head", &head])["reports"],
        json!(["/reviews/own/report"])
    );
}

#[test]
fn a_head_git_cannot_resolve_is_refused_and_one_it_can_is_checked() {
    let repo = Repo::new();
    for flags in [
        ["--base", "main", "--head", "no-such-branch"],
        ["--base", "no-such-branch", "--head", "main"],
    ] {
        let (code, document) = repo.call("covered", &flags);
        assert_eq!(
            (code, &document["code"]),
            (1, &json!("review::range::unknown")),
            "{flags:?}"
        );
    }
    repo.git(&["checkout", "-q", "-b", "change"]);
    repo.commit("src/a.txt", "one\n");
    uncovered(&repo, &["--head", "HEAD"]);
}

#[test]
fn a_call_without_a_base_is_refused_and_one_with_it_is_checked() {
    let repo = Repo::new();
    reviewed(&repo);
    let (code, document) = repo.call("covered", &[]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("board::plugin::bad_input"))
    );
    covered(&repo, &[]);
}

#[test]
fn a_report_of_only_the_tip_is_refused_and_one_of_every_commit_since_the_base_admitted() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let first = repo.commit("src/a.txt", "one\n");
    let head = repo.commit("src/b.txt", "two\n");
    report(&repo, "tip", &first, &head, &json!({}));
    assert_eq!(uncovered(&repo, &[])["merge_base"], json!(base));
    let whole = report(&repo, "whole", &base, &head, &json!({}));
    let reports = covered(&repo, &[])["reports"].clone();
    assert_eq!(reports.as_array().unwrap().last(), Some(&json!(whole)));
}

#[test]
fn a_report_asking_for_changes_is_refused_and_one_approving_admitted() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let head = repo.commit("src/a.txt", "one\n");
    report(&repo, "first", &base, &head, &json!({"verdict": "changes"}));
    uncovered(&repo, &[]);
    let second = report(&repo, "second", &base, &head, &json!({}));
    assert_eq!(covered(&repo, &[])["reports"], json!([second]));
}

#[test]
fn a_commit_adding_a_file_that_is_not_a_record_is_not_skipped_and_one_adding_only_records_is() {
    let repo = Repo::new();
    let (_, head, _) = reviewed(&repo);
    let note = repo.commit("reviews/notes.md", "A note.\n");
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([note]));
    repo.git(&["rm", "-q", "reviews/notes.md"]);
    repo.git(&["commit", "-qm", "drop the note"]);
    uncovered(&repo, &[]);
    repo.git(&["reset", "-q", "--hard", "HEAD~2"]);
    repo.brief("later", &format!("{head}..{head}"), &json!({}));
    assert_eq!(covered(&repo, &[])["skipped"], 3);
}

#[test]
fn a_commit_changing_a_review_record_is_not_skipped_and_one_adding_a_record_is() {
    let repo = Repo::new();
    let (_, head, path) = reviewed(&repo);
    let brief = record(
        repo.get("/reviews/change/brief"),
        &json!({"focus": "Something else."}),
    );
    let changed = repo.commit("reviews/change/brief.json", &brief);
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([changed]));
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.brief("later", &format!("{head}..{head}"), &json!({}));
    let output = covered(&repo, &[]);
    assert_eq!(
        (&output["skipped"], &output["reports"]),
        (&json!(3), &json!([path]))
    );
}

#[test]
fn a_commit_setting_only_a_reports_source_is_skipped_and_one_also_changing_its_findings_is_not() {
    let repo = Repo::new();
    let (_, _, path) = reviewed(&repo);
    let source = json!({"source": "https://github.com/o/r/pull/7#issuecomment-1"});
    let both = record(
        repo.get(&path),
        &json!({"source": source["source"], "findings": [{"severity": "low", "text": "Later."}]}),
    );
    let changed = repo.commit("reviews/change/report.json", &both);
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([changed]));
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.commit(
        "reviews/change/report.json",
        &record(repo.get(&path), &source),
    );
    let output = covered(&repo, &[]);
    assert_eq!(
        (&output["skipped"], &output["reports"]),
        (&json!(3), &json!([path]))
    );
}

#[test]
fn a_commit_adding_a_record_of_another_type_or_at_another_path_is_not_skipped_and_a_brief_at_its_path_is()
 {
    let repo = Repo::new();
    let (_, head, _) = reviewed(&repo);
    let brief =
        json!({"$type": "review/brief/v1", "range": format!("{head}..{head}"), "focus": "Later."});
    let other_type = repo.commit(
        "reviews/later/brief.json",
        &record(brief.clone(), &json!({"$type": "review/report/v1"})),
    );
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([other_type]));
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let other_path = repo.commit("reviews/later.json", &brief.to_string());
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([other_path]));
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.commit("reviews/later/brief.json", &brief.to_string());
    assert_eq!(covered(&repo, &[])["skipped"], 3);
}

#[test]
fn a_branch_that_drops_a_reviewed_commit_is_refused_and_one_that_keeps_both_admitted() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    repo.commit("src/a.txt", "one\nextra\n");
    let undone = repo.commit("src/a.txt", "one\n");
    let ids = patch_ids(&repo, "ids", &base, &undone);
    repo.git(&["reset", "-q", "--hard", &undone]);
    let path = report(&repo, "both", &base, &base, &json!({"patch_ids": ids}));
    let record = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("other.txt", "unrelated\n");
    repo.git(&["checkout", "-q", "-b", "dropped"]);
    let kept = repo.commit("src/a.txt", "one\nextra\n");
    repo.git(&["cherry-pick", &record]);
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([kept]));
    repo.git(&["checkout", "-q", "-b", "both", "main"]);
    repo.commit("src/a.txt", "one\nextra\n");
    repo.commit("src/a.txt", "one\n");
    repo.git(&["cherry-pick", &record]);
    assert_eq!(covered(&repo, &[])["reports"], json!([path]));
}

#[test]
fn a_report_changed_on_disk_is_ignored_and_the_committed_one_counts() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let head = repo.commit("src/a.txt", "one\n");
    let path = report(&repo, "manual", &base, &head, &json!({}));
    let file = "reviews/manual/report.json";
    let committed = std::fs::read_to_string(repo.root().join(file)).unwrap();
    let changed = record(
        serde_json::from_str(&committed).unwrap(),
        &json!({"by": {"person": "someone-else"}}),
    );
    repo.write(file, &changed);
    assert_eq!(uncovered(&repo, &[])["uncovered"], json!([head]));
    repo.write(file, &committed);
    assert_eq!(covered(&repo, &[])["reports"], json!([path]));
}

/// A project mounted at /project whose second commit no report covers
/// yet, with the changes plugin's schema in the board.
struct Mounted {
    project: Scenario,
    base: String,
    head: String,
}

impl Mounted {
    /// The project's commits are only in its own clone, so `covered` finds
    /// them only through the mount.
    fn new(repo: &Repo) -> Self {
        repo.write(".board/types/changes/change/v2.schema.json", CHANGE_SCHEMA);
        repo.write(".board/types/changes/plan/v1.schema.json", PLAN_SCHEMA);
        let (project, mount) = library(&[("README.md", "A project.\n")]);
        repo.commit("project.json", &mount.to_string());
        let mut mounted = Self {
            project,
            base: mount["commit"].as_str().unwrap().to_owned(),
            head: String::new(),
        };
        mounted.head = mounted.commit("src.txt", "src.txt\n");
        mounted
    }

    /// Commits `text` at `name` in the project's clone and returns the
    /// commit.
    fn commit(&self, name: &str, text: &str) -> String {
        self.project.write(name, Item::File(text));
        commit_all(self.project.workdir(), &format!("add {name}"))
    }
}

/// Writes the change /changes/work on `mount` at `head`, or with no head,
/// with a plan that attaches `records`, and returns its path.
fn change(repo: &Repo, head: Option<&str>, records: &[&str], mount: &str) -> String {
    let mut record = json!({
        "$type": "changes/change/v2", "title": "Work", "mount": mount, "phase": "review",
        "plan": "/changes/work/plans/1",
    });
    if let Some(head) = head {
        record["head"] = json!(head);
    }
    let plan = json!({
        "$type": "changes/plan/v1", "text": "/changes/work/plans/1.md", "uses": [], "records": records,
    });
    repo.write("changes/work/plans/1.md", "# Work\n");
    repo.write("changes/work/plans/1.json", &plan.to_string());
    repo.write("changes/work.json", &record.to_string());
    "/changes/work".to_owned()
}

/// Writes, without committing, a person's report of `base..head` beside
/// the change, which approves with no findings and no patch ids unless
/// `fields` say otherwise, and returns its path.
fn attached(repo: &Repo, name: &str, base: &str, head: &str, fields: &Value) -> String {
    let record = record(
        json!({
            "$type": "review/report/v1", "range": format!("{base}..{head}"), "patch_ids": [],
            "findings": [], "verdict": "approve", "by": {"person": "a-person"},
        }),
        fields,
    );
    repo.write(&format!("changes/work/reviews/{name}.json"), &record);
    format!("/changes/work/reviews/{name}")
}

fn finding(severity: &str) -> Value {
    json!({"findings": [{"severity": severity, "text": "A finding."}]})
}

/// `covered --base <base> --change /changes/work <flags>`.
fn of_change(repo: &Repo, base: &str, flags: &[&str]) -> (u8, Value) {
    let flags: Vec<&str> = ["--base", base, "--change", "/changes/work"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    repo.call("covered", &flags)
}

#[test]
fn a_report_the_change_does_not_attach_is_refused_and_an_attached_one_covers_the_mounts_commits() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, Some(&mounted.head), &[], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::head::uncovered")),
        "{document}"
    );
    assert_eq!(document["fields"]["uncovered"], json!([mounted.head]));
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("changes.attach --path /changes/work"),
        "{message}"
    );
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let (code, output) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(code, 0, "{output}");
    assert_eq!(
        output,
        json!({"head": mounted.head, "merge_base": mounted.base, "skipped": 0, "commits": 1, "reports": [path]})
    );
}

#[test]
fn a_change_that_is_not_there_is_refused_and_one_that_is_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let flags = ["--base", &mounted.base, "--change", "/changes/other"];
    let (code, document) = repo.call("covered", &flags);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::change::not_found")),
        "{document}"
    );
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn a_change_whose_mount_names_no_clone_is_refused_and_one_on_a_mount_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    for mount in ["/nowhere", "/changes", "/project/README.md"] {
        change(&repo, Some(&mounted.head), &[&path], mount);
        let (code, document) = of_change(&repo, &mounted.base, &[]);
        assert_eq!(
            (code, &document["code"]),
            (1, &json!("review::mount::not_found")),
            "{mount}: {document}"
        );
    }
    change(&repo, Some(&mounted.head), &[&path], "/project");
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn a_change_with_no_head_is_refused_and_one_with_a_head_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, None, &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::change::no_head")),
        "{document}"
    );
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("changes.attach --path /changes/work --head"),
        "{message}"
    );
    change(&repo, Some(&mounted.head), &[&path], "/project");
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn the_change_head_past_the_report_is_refused_and_a_head_flag_naming_the_reviewed_one_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let later = mounted.commit("later.txt", "later.txt\n");
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, Some(&later), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["uncovered"]),
        (1, &json!("review::head::uncovered"), &json!([later]))
    );
    let (code, output) = of_change(&repo, &mounted.base, &["--head", &mounted.head]);
    assert_eq!(
        (code, &output["head"]),
        (0, &json!(mounted.head)),
        "{output}"
    );
}

#[test]
fn an_attached_report_with_a_high_finding_is_refused_and_one_with_a_low_finding_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &finding("high"));
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["reports"]),
        (1, &json!("review::head::uncovered"), &json!(1))
    );
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("of 1 reports, 0 ask for changes and 1 have a critical or high finding"),
        "{message}"
    );
    attached(&repo, "one", &mounted.base, &mounted.head, &finding("low"));
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn a_project_commit_adding_only_a_report_is_checked_with_a_change_and_covered_once_reviewed() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let text = json!({
        "$type": "review/report/v1", "range": format!("{}..{}", mounted.base, mounted.head), "patch_ids": [],
        "findings": [], "verdict": "approve", "by": {"person": "a-person"},
    })
    .to_string();
    let added = mounted.commit("reviews/x/report.json", &text);
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, Some(&added), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["uncovered"]),
        (1, &json!("review::head::uncovered"), &json!([added]))
    );
    attached(&repo, "one", &mounted.base, &added, &json!({}));
    let (code, output) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &output["skipped"], &output["commits"]),
        (0, &json!(0), &json!(2)),
        "{output}"
    );
}

#[test]
fn a_report_attached_twice_is_counted_once_when_refused_and_listed_once_when_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let later = mounted.commit("later.txt", "later.txt\n");
    let path = attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    change(&repo, Some(&later), &[&path, &path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["reports"]),
        (1, &json!("review::head::uncovered"), &json!(1)),
        "{document}"
    );
    let (code, output) = of_change(&repo, &mounted.base, &["--head", &mounted.head]);
    assert_eq!((code, &output["reports"]), (0, &json!([path])), "{output}");
}

#[test]
fn a_report_asking_for_changes_with_a_high_finding_is_counted_once_and_an_approving_low_one_admitted()
 {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let both = record(finding("high"), &json!({"verdict": "changes"}));
    let path = attached(
        &repo,
        "one",
        &mounted.base,
        &mounted.head,
        &serde_json::from_str(&both).unwrap(),
    );
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::head::uncovered")),
        "{document}"
    );
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains("of 1 reports, 1 ask for changes and 0 have a critical or high finding"),
        "{message}"
    );
    attached(&repo, "one", &mounted.base, &mounted.head, &finding("low"));
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn an_attached_report_asking_for_changes_is_refused_and_an_approving_one_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(
        &repo,
        "one",
        &mounted.base,
        &mounted.head,
        &json!({"verdict": "changes"}),
    );
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"]),
        (1, &json!("review::head::uncovered")),
        "{document}"
    );
    attached(&repo, "one", &mounted.base, &mounted.head, &json!({}));
    assert_eq!(of_change(&repo, &mounted.base, &[]).0, 0);
}

#[test]
fn an_attached_report_without_the_heads_patch_id_is_refused_and_one_listing_it_admitted() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let path = attached(&repo, "one", &mounted.base, &mounted.base, &json!({}));
    change(&repo, Some(&mounted.head), &[&path], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["uncovered"]),
        (1, &json!("review::head::uncovered"), &json!([mounted.head]))
    );
    let id = project_patch_id(mounted.project.workdir(), &mounted.head);
    attached(
        &repo,
        "one",
        &mounted.base,
        &mounted.base,
        &json!({"patch_ids": [id]}),
    );
    let (code, output) = of_change(&repo, &mounted.base, &[]);
    assert_eq!((code, &output["reports"]), (0, &json!([path])), "{output}");
}

/// The patch id of `commit` in the repository at `root`, as plan/review.md
/// defines it.
#[expect(
    clippy::disallowed_methods,
    reason = "the expected patch id comes from git's own pipeline, as a person runs it"
)]
fn project_patch_id(root: &Path, commit: &str) -> String {
    let command = format!(
        "git diff-tree -p --no-renames --no-ext-diff --no-textconv --full-index {commit} | git patch-id --verbatim"
    );
    let ran = std::process::Command::new("sh")
        .args(["-c", &command])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    text(&ran.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn an_attached_report_that_is_not_there_is_refused_and_admitted_once_written() {
    let repo = Repo::new();
    let mounted = Mounted::new(&repo);
    let missing = "/changes/work/reviews/later";
    change(&repo, Some(&mounted.head), &[missing], "/project");
    let (code, document) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["reports"]),
        (1, &json!("review::head::uncovered"), &json!(0)),
        "{document}"
    );
    attached(&repo, "later", &mounted.base, &mounted.head, &json!({}));
    let (code, output) = of_change(&repo, &mounted.base, &[]);
    assert_eq!(
        (code, &output["reports"]),
        (0, &json!([missing])),
        "{output}"
    );
}

#[test]
fn a_commit_whose_patch_is_larger_than_board_keeps_of_a_process_is_covered_by_its_range() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    // More than the 16 MiB of stdout Board keeps of a process.
    let large = "a line of a large generated file\n".repeat(600_000);
    let head = repo.commit("src/large.txt", &large);
    let (code, refused) = repo.call("covered", &["--base", "main"]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("review::head::uncovered")),
        "{refused}"
    );
    let path = report(&repo, "large", &base, &head, &json!({}));
    let output = covered(&repo, &[]);
    assert_eq!(
        (&output["commits"], &output["reports"]),
        (&json!(1), &json!([path]))
    );
}

#[test]
fn git_printing_a_hint_on_stderr_does_not_stop_a_range_being_covered() {
    let repo = Repo::new();
    let (base, head, _) = reviewed(&repo);
    // git prints hints like this one for a repository with a grafts file,
    // and exits 0.
    let git = stand_in_git(r#"echo "hint: support for grafts is deprecated" >&2"#);
    repo.bin.write("git", Item::Executable(&git));
    let output = covered(&repo, &[]);
    assert_eq!(output["commits"], json!(2), "{output} {base} {head}");
}

#[test]
fn git_diff_tree_failing_inside_the_patch_id_pipeline_is_refused_and_named() {
    let repo = Repo::new();
    reviewed(&repo);
    let git = stand_in_git(
        r#"case " $* " in *" diff-tree --stdin "*) echo "fatal: bad object" >&2; exit 128;; esac"#,
    );
    repo.bin.write("git", Item::Executable(&git));
    let (code, refused) = repo.call("covered", &["--base", "main"]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("review::git::failed")),
        "{refused}"
    );
    let message = refused["message"].as_str().unwrap();
    assert!(
        message.starts_with("`git diff-tree` failed in "),
        "{message}"
    );
    assert!(message.contains(": fatal: bad object; "), "{message}");
}
