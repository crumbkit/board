//! `review.post` asks the owner with the whole comment, posts it with the
//! stand-in gh once the owner approves, and records where it went. The
//! owner answers with `board answer`, which the test signer signs, where
//! the Python tests ran the plugin's decide as the owner.

use super::*;

const PR: &str = "https://github.com/o/r/pull/7";

fn range() -> String {
    format!("{}..{}", "a".repeat(40), "b".repeat(40))
}

fn report() -> Value {
    json!({
        "$type": "review/report/v1",
        "range": range(),
        "patch_ids": [],
        "findings": [
            {"severity": "high", "file": "src/sync.rs", "line": 3, "text": "Ask @alice about the lock."},
            {"severity": "low", "text": "The change has no test."},
        ],
        "verdict": "changes",
        "by": "claude-opus",
    })
}

fn expected() -> String {
    format!(
        "**Review: changes** by claude-opus\n\nCommits `{}`.\n\n- **high** `src/sync.rs:3` Ask @\u{200d}alice about the lock.\n- **low** The change has no test.\n\n---\nReport /reviews/r1/report, for {PR}.\n",
        range()
    )
}

impl Repo {
    /// Commits a report at /reviews/r1/report holding `report()` with
    /// `fields`.
    fn reported(&self, fields: &Value) {
        self.commit("reviews/r1/report.json", &record(report(), fields));
    }

    fn post_to(&self, pr: &str) -> (u8, Value) {
        self.call("post", &["--report", "/reviews/r1/report", "--pr", pr])
    }

    fn post(&self) -> (u8, Value) {
        self.post_to(PR)
    }

    /// The effect a refused post asked the owner about.
    fn asked(&self) -> String {
        let (code, asked) = self.post();
        assert_eq!(code, 1, "{asked}");
        assert_eq!(asked["code"], "review::board::refused", "{asked}");
        assert_eq!(asked["fields"]["code"], "board::settings::asked", "{asked}");
        asked["fields"]["effect"].as_str().unwrap().to_owned()
    }

    fn gh_calls(&self) -> usize {
        std::fs::read_to_string(self.bin.workdir().join("gh-calls.txt"))
            .map_or(0, |calls| calls.lines().count())
    }

    fn gh_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.bin.workdir().join(name)).unwrap()
    }
}

#[test]
fn a_first_post_asks_the_owner_with_the_whole_comment_and_an_approved_one_posts_that_comment() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let (code, asked) = repo.post();
    assert_eq!(code, 1);
    assert_eq!(asked["code"], "review::board::refused");
    assert_eq!(asked["fields"]["code"], "board::settings::asked");
    let effect = asked["fields"]["effect"].as_str().unwrap();
    let message = asked["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("pending effect {effect}")),
        "{message}"
    );
    assert_eq!(repo.personal.pending(), [effect]);
    let pending = repo
        .personal
        .record(&format!("/effects/pending/{effect}"))
        .unwrap();
    assert_eq!(
        (&pending["op"], &pending["path"], &pending["text"]),
        (
            &json!("review.post"),
            &json!("/reviews/r1/report"),
            &json!(expected())
        )
    );
    assert_eq!(repo.gh_calls(), 0);

    repo.personal.answer(effect, "approve");
    let (code, posted) = repo.post();
    assert_eq!(code, 0, "{posted}");
    assert_eq!(posted, json!({"comment": COMMENT_URL, "posted": true}));
    assert_eq!(
        repo.gh_file("gh-argv.txt").lines().collect::<Vec<_>>(),
        ["pr", "comment", PR, "--body-file", "-"]
    );
    assert_eq!(repo.gh_file("gh-stdin.txt"), expected());
    assert_eq!(
        repo.get("/reviews/r1/report"),
        record(report(), &json!({"source": COMMENT_URL}))
            .parse::<Value>()
            .unwrap()
    );
}

#[test]
fn a_post_the_owner_refused_is_refused_and_one_not_yet_answered_is_asked() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let effect = repo.asked();
    repo.personal.answer(&effect, "refuse");
    let (code, refused) = repo.post();
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "review::board::refused");
    assert_eq!(refused["fields"]["code"], "board::settings::refused");
    assert_eq!(refused["fields"]["effect"], effect);
    assert_eq!(repo.gh_calls(), 0);
}

#[test]
fn a_post_whose_report_has_a_source_skips_and_one_without_a_source_posts() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let effect = repo.asked();
    repo.personal.answer(&effect, "approve");
    assert_eq!(repo.post().1["posted"], true);
    let (code, again) = repo.post();
    assert_eq!(code, 0, "{again}");
    assert_eq!(again, json!({"comment": COMMENT_URL, "posted": false}));
    assert_eq!(repo.gh_calls(), 1);
}

#[test]
fn a_post_whose_gh_fails_records_no_source_and_a_rerun_posts() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let effect = repo.asked();
    repo.personal.answer(&effect, "approve");
    repo.bin.write("gh-exit.txt", Item::File("1"));
    let (code, failed) = repo.post();
    assert_eq!(code, 1);
    assert_eq!(failed["code"], "review::gh::failed");
    assert_eq!(failed["fields"]["exit"], 1);
    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("HTTP 502: Bad Gateway"), "{message}");
    assert_eq!(repo.get("/reviews/r1/report").get("source"), None);
    repo.bin.write("gh-exit.txt", Item::File("0"));
    let (code, posted) = repo.post();
    assert_eq!(code, 0, "{posted}");
    assert_eq!(posted["posted"], true);
    assert_eq!(repo.gh_calls(), 2);
}

#[test]
fn a_changed_comment_asks_again_under_a_new_effect_and_an_unchanged_one_asks_under_the_same() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let first = repo.asked();
    assert_eq!(repo.asked(), first);
    assert_eq!(repo.personal.pending().len(), 1);
    repo.reported(&json!({"verdict": "comment"}));
    let second = repo.asked();
    assert_ne!(second, first);
    assert_eq!(repo.personal.pending().len(), 2);
}

#[test]
fn a_post_to_another_pull_request_asks_again_under_a_new_effect() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let first = repo.asked();
    let (_, other) = repo.post_to("https://github.com/o/r/pull/8");
    assert_ne!(other["fields"]["effect"], json!(first));
}

#[test]
fn a_post_whose_gh_prints_another_url_records_no_source_and_one_printing_the_comment_posts() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    let effect = repo.asked();
    repo.personal.answer(&effect, "approve");
    repo.bin.write(
        "gh-url.txt",
        Item::File("https://github.com/o/r/pull/70#issuecomment-1\n"),
    );
    let (code, failed) = repo.post();
    assert_eq!((code, &failed["code"]), (1, &json!("review::gh::failed")));
    let message = failed["message"].as_str().unwrap();
    assert!(message.contains("may have posted the comment"), "{message}");
    assert_eq!(repo.get("/reviews/r1/report").get("source"), None);
    repo.bin.write("gh-url.txt", Item::Deleted);
    assert_eq!(
        repo.post(),
        (0, json!({"comment": COMMENT_URL, "posted": true}))
    );
}

#[test]
fn a_report_that_is_not_there_is_refused_as_not_found_and_one_that_is_there_is_asked() {
    let repo = Repo::new();
    let (code, missing) = repo.post();
    assert_eq!(
        (code, &missing["code"]),
        (1, &json!("review::report::not_found"))
    );
    repo.reported(&json!({}));
    repo.asked();
}

#[test]
fn a_report_its_schema_refuses_is_refused_as_not_found_before_asking_and_a_valid_one_is_asked() {
    let repo = Repo::new();
    repo.reported(&json!({"by": ""}));
    let (code, invalid) = repo.post();
    assert_eq!(
        (code, &invalid["code"]),
        (1, &json!("review::report::not_found"))
    );
    assert_eq!(repo.personal.pending(), Vec::<String>::new());
    repo.reported(&json!({}));
    repo.asked();
}

#[test]
fn a_pull_request_that_is_not_a_github_pull_request_url_is_refused_and_one_that_is_is_asked() {
    let repo = Repo::new();
    repo.reported(&json!({}));
    for pr in ["7", "https://github.com/o/r#x/pull/7"] {
        let (code, refused) = repo.post_to(pr);
        assert_eq!(
            (code, &refused["code"]),
            (1, &json!("board::plugin::bad_input")),
            "{pr}"
        );
    }
    repo.asked();
}
