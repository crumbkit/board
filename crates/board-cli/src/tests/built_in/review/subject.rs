//! A brief and a report can name a `subject`, a record pinned by its blob,
//! in place of a commit range.

use super::*;

const INVALID: &str = "board::record::invalid";
const PLAN: &str = "notes/plan.md";
const FOCUS: &str = "Check the plan states only what is new.";

fn commits() -> String {
    format!("{}..{}", "a".repeat(40), "b".repeat(40))
}

/// Commits `text` at `path` and returns a subject pinning it at its blob.
fn pinned(repo: &Repo, path: &str, text: &str) -> Value {
    repo.commit(path, text);
    json!({"record": format!("/{path}"), "blob": repo.git(&["hash-object", path])})
}

fn plan(repo: &Repo) -> Value {
    pinned(repo, PLAN, "# Plan\n\nShip it.\n")
}

/// Commits a brief at /reviews/<name>/brief holding a focus and `fields`,
/// and returns its path.
fn brief(repo: &Repo, name: &str, fields: &Value) -> String {
    let base = json!({"$type": "review/brief/v1", "focus": FOCUS});
    repo.commit(&format!("reviews/{name}/brief.json"), &record(base, fields));
    format!("/reviews/{name}/brief")
}

/// Commits a person's approving report at /reviews/<name>/report holding
/// `fields`, and returns its path.
fn report(repo: &Repo, name: &str, fields: &Value) -> String {
    let base = json!({"$type": "review/report/v1", "findings": [], "verdict": "approve", "by": "a person"});
    repo.commit(
        &format!("reviews/{name}/report.json"),
        &record(base, fields),
    );
    format!("/reviews/{name}/report")
}

fn expected(entries: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
    codes(entries)
}

#[test]
fn a_brief_names_a_range_or_a_subject_and_one_with_both_or_neither_is_refused() {
    let repo = Repo::new();
    let subject = plan(&repo);
    brief(&repo, "range", &json!({"range": commits()}));
    brief(&repo, "subject", &json!({"subject": subject}));
    brief(
        &repo,
        "both",
        &json!({"range": commits(), "subject": subject}),
    );
    brief(&repo, "neither", &json!({}));
    assert_eq!(
        repo.checked(),
        expected(&[
            ("/reviews/both/brief", Some(INVALID)),
            ("/reviews/neither/brief", Some(INVALID)),
            ("/reviews/range/brief", None),
            ("/reviews/subject/brief", None),
        ])
    );
}

#[test]
fn a_report_holds_a_range_with_patch_ids_or_a_subject_and_no_mix_of_them() {
    let repo = Repo::new();
    let subject = plan(&repo);
    report(
        &repo,
        "range",
        &json!({"range": commits(), "patch_ids": []}),
    );
    report(&repo, "subject", &json!({"subject": subject}));
    report(
        &repo,
        "both",
        &json!({"range": commits(), "patch_ids": [], "subject": subject}),
    );
    report(&repo, "range-alone", &json!({"range": commits()}));
    report(
        &repo,
        "range-subject",
        &json!({"range": commits(), "subject": subject}),
    );
    report(
        &repo,
        "ids-subject",
        &json!({"patch_ids": [], "subject": subject}),
    );
    report(&repo, "neither", &json!({}));
    assert_eq!(
        repo.checked(),
        expected(&[
            ("/reviews/both/report", Some(INVALID)),
            ("/reviews/ids-subject/report", Some(INVALID)),
            ("/reviews/neither/report", Some(INVALID)),
            ("/reviews/range-alone/report", Some(INVALID)),
            ("/reviews/range-subject/report", Some(INVALID)),
            ("/reviews/range/report", None),
            ("/reviews/subject/report", None),
        ])
    );
}

#[test]
fn a_subject_holds_a_hex_blob_and_nothing_else() {
    let repo = Repo::new();
    let subject = plan(&repo);
    let blob = subject["blob"].as_str().unwrap();
    brief(&repo, "whole", &json!({"subject": subject}));
    brief(
        &repo,
        "short",
        &json!({"subject": {"record": subject["record"], "blob": &blob[..12]}}),
    );
    brief(
        &repo,
        "extra",
        &json!({"subject": {"record": subject["record"], "blob": blob, "range": commits()}}),
    );
    brief(
        &repo,
        "no-blob",
        &json!({"subject": {"record": subject["record"]}}),
    );
    assert_eq!(
        repo.checked(),
        expected(&[
            ("/reviews/extra/brief", Some(INVALID)),
            ("/reviews/no-blob/brief", Some(INVALID)),
            ("/reviews/short/brief", Some(INVALID)),
            ("/reviews/whole/brief", None),
        ])
    );
}

#[test]
fn a_run_gives_the_agent_the_pinned_record_and_its_references_and_the_report_names_the_subject() {
    let repo = Repo::new();
    let first = report(
        &repo,
        "first",
        &json!({"range": commits(), "patch_ids": [], "brief": "/reviews/plan/brief"}),
    );
    let subject =
        json!({"record": first, "blob": repo.git(&["hash-object", "reviews/first/report.json"])});
    let path = brief(&repo, "plan", &json!({"subject": subject}));
    let (code, output) = repo.call("run", &["--brief", &path]);
    assert_eq!(code, 0, "{output}");
    assert_eq!(
        output,
        json!({"report": "/reviews/plan/report", "subject": subject, "verdict": "approve", "findings": 0})
    );
    let written = repo.get("/reviews/plan/report");
    assert_eq!(
        (&written["subject"], &written["brief"]),
        (&subject, &json!(path))
    );
    assert_eq!(
        (written.get("range"), written.get("patch_ids")),
        (None, None)
    );
    let prompt = repo.prompt();
    let held = std::fs::read_to_string(repo.root().join("reviews/first/report.json")).unwrap();
    for expected in [
        format!("Record: {first} at {}", subject["blob"].as_str().unwrap()),
        "- /brief: /reviews/plan/brief".to_owned(),
        held,
    ] {
        assert!(prompt.contains(&expected), "{expected}: {prompt}");
    }
    assert!(!prompt.contains("## Diff"), "{prompt}");
    assert_eq!(repo.run(&["check"], &[]).code, 0);
}

#[test]
fn a_subject_that_moved_after_its_brief_is_refused_and_one_at_its_pinned_blob_runs() {
    let repo = Repo::new();
    let subject = plan(&repo);
    let path = brief(&repo, "plan", &json!({"subject": subject}));
    repo.commit(PLAN, "# Plan\n\nShip it later.\n");
    let (code, refused) = repo.call("run", &["--brief", &path]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("review::subject::moved")),
        "{refused}"
    );
    let current = repo.git(&["hash-object", PLAN]);
    let fields = &refused["fields"];
    assert_eq!(
        (&fields["record"], &fields["pinned"], &fields["current"]),
        (&subject["record"], &subject["blob"], &json!(current))
    );
    let repinned = brief(
        &repo,
        "plan-2",
        &json!({"subject": {"record": subject["record"], "blob": current}}),
    );
    let (code, output) = repo.call("run", &["--brief", &repinned]);
    assert_eq!(code, 0, "{output}");
    assert!(repo.prompt().contains("Ship it later."));
}

#[test]
fn a_subject_with_nothing_there_is_refused_and_one_that_is_there_runs() {
    let repo = Repo::new();
    let subject = plan(&repo);
    let gone = brief(
        &repo,
        "gone",
        &json!({"subject": {"record": "/notes/missing.md", "blob": subject["blob"]}}),
    );
    let (code, refused) = repo.call("run", &["--brief", &gone]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("review::subject::not_found")),
        "{refused}"
    );
    let there = brief(&repo, "there", &json!({"subject": subject}));
    assert_eq!(repo.call("run", &["--brief", &there]).0, 0);
}

#[test]
fn a_subject_report_never_covers_a_commit_and_a_range_report_of_the_same_commit_does() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let subject = plan(&repo);
    let head = repo.git(&["rev-parse", "HEAD"]);
    report(&repo, "plan", &json!({"subject": subject}));
    let (code, refused) = repo.call("covered", &["--base", "main"]);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("review::head::uncovered")),
        "{refused}"
    );
    assert_eq!(
        (
            &refused["fields"]["reports"],
            &refused["fields"]["uncovered"]
        ),
        (&json!(0), &json!([head]))
    );
    let path = report(
        &repo,
        "code",
        &json!({"range": format!("{base}..{head}"), "patch_ids": []}),
    );
    let (code, output) = repo.call("covered", &["--base", "main"]);
    assert_eq!((code, &output["reports"]), (0, &json!([path])), "{output}");
}
