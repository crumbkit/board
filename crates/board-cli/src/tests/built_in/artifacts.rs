//! The artifacts plugin through the binary: what `list` returns and filters,
//! what `import`, `record`, `set` and `agree` write, their refusals in
//! pairs, and an answer from a cold index. The list and import cases were
//! the pytest suite of the Python plugin in crumbkit/board#46.

use super::*;

const A: &str = "https://claude.ai/artifact/A";
const B: &str = "https://claude.ai/artifact/B";
const C: &str = "https://claude.ai/code/artifact/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21";
const ACCOUNT: &str = "owner@example.com";

fn board() -> Fixture {
    Fixture::enabling(&["artifacts"])
}

/// An `artifacts/artifact/v1` record of `url`, with `fields` added or
/// replacing its own.
fn artifact(url: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "artifacts/artifact/v1",
            "url": url,
            "account": ACCOUNT,
            "first_seen": "2026-10-31T18:20:00Z",
            "state": "draft",
        }),
        fields,
    )
}

/// An `artifacts/version/v1` record of a published page recorded `at`.
fn version(at: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "artifacts/version/v1",
            "version": "1793473476-1c42",
            "title": "Plan",
            "sha256": "0".repeat(64),
            "source": "/work/scratch/plan.html",
            "session": "session-1",
            "at": at,
        }),
        fields,
    )
}

/// The output of `board call artifacts.<capability>` with `input` on stdin.
fn given(board: &Fixture, capability: &str, input: &Value) -> Value {
    let target = format!("artifacts.{capability}");
    let output = board.run_with(&["--json", "call", &target], &input.to_string());
    assert_eq!(output.code, 0, "{}", text(&output.stdout));
    json(&output)["data"]["output"].clone()
}

/// The code `board call artifacts.<capability>` refuses `input` with.
fn refused_given(board: &Fixture, capability: &str, input: &Value) -> String {
    let target = format!("artifacts.{capability}");
    let output = board.run_with(&["--json", "call", &target], &input.to_string());
    assert_ne!(output.code, 0, "{}", text(&output.stdout));
    json(&output)["code"].as_str().unwrap().to_owned()
}

/// The paths `artifacts.list` returns with `flags`, in order.
fn listed(board: &Fixture, flags: &[&str]) -> Vec<String> {
    let output = board.call("artifacts.list", flags);
    (output["artifacts"].as_array().unwrap().iter())
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect()
}

/// The record at `path` from the board's root.
fn stored(board: &Fixture, path: &str) -> Value {
    serde_json::from_str(&board.read(path)).unwrap()
}

#[test]
fn list_gives_each_artifact_every_version_newest_first() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({})))
        .write(
            "artifacts/A/versions/1.json",
            &version(
                "2026-10-01T09:00:00Z",
                &json!({"version": "1", "title": "Old"}),
            ),
        )
        .write("artifacts/A/versions/1.html", "<p>old</p>")
        .write(
            "artifacts/A/versions/2.json",
            &version(
                "2026-10-03T09:00:00Z",
                &json!({"version": "2", "title": "New"}),
            ),
        )
        .write(
            "artifacts/B.json",
            &artifact(B, &json!({"first_seen": "2026-10-02T09:00:00Z"})),
        );
    let output = board.call("artifacts.list", &[]);
    assert_eq!(
        output,
        json!({
            "artifacts": [
                {
                    "path": "/artifacts/A",
                    "url": A,
                    "account": ACCOUNT,
                    "first_seen": "2026-10-31T18:20:00Z",
                    "subject": [],
                    "shows": null,
                    "state": "draft",
                    "agreed_by": null,
                    "superseded_by": null,
                    "area": [],
                    "versions": [
                        {"path": "/artifacts/A/versions/2", "version": "2", "title": "New", "at": "2026-10-03T09:00:00Z"},
                        {"path": "/artifacts/A/versions/1", "version": "1", "title": "Old", "at": "2026-10-01T09:00:00Z"},
                    ],
                },
                {
                    "path": "/artifacts/B",
                    "url": B,
                    "account": ACCOUNT,
                    "first_seen": "2026-10-02T09:00:00Z",
                    "subject": [],
                    "shows": null,
                    "state": "draft",
                    "agreed_by": null,
                    "superseded_by": null,
                    "area": [],
                    "versions": [],
                },
            ],
            "total": 2,
            "truncated": false,
            "unreadable": 0,
        })
    );
}

/// A version record keyed by its page's hash, with no version id.
fn hashed() -> String {
    let mut record: Value =
        serde_json::from_str(&version("2026-10-31T18:20:00Z", &json!({}))).unwrap();
    record.as_object_mut().unwrap().remove("version");
    record.to_string()
}

#[test]
fn a_version_keyed_by_its_hash_lists_no_version_id() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({})))
        .write("artifacts/A/versions/0123456789abcdef.json", &hashed());
    let output = board.call("artifacts.list", &[]);
    let newest = &output["artifacts"][0]["versions"][0];
    assert_eq!(
        (&newest["path"], &newest["version"]),
        (
            &json!("/artifacts/A/versions/0123456789abcdef"),
            &Value::Null
        )
    );
}

#[test]
fn list_keeps_one_account_and_stops_at_the_limit() {
    let board = board();
    board
        .write(
            "artifacts/A.json",
            &artifact(A, &json!({"first_seen": "2026-10-01T09:00:00Z"})),
        )
        .write(
            "artifacts/B.json",
            &artifact(
                B,
                &json!({"first_seen": "2026-10-02T09:00:00Z", "account": "work@example.com"}),
            ),
        )
        .write(
            "artifacts/C.json",
            &artifact(C, &json!({"first_seen": "2026-10-03T09:00:00Z"})),
        );
    let mine = board.call("artifacts.list", &["--account", ACCOUNT, "--limit", "1"]);
    assert_eq!(mine["artifacts"][0]["path"], "/artifacts/C");
    assert_eq!(
        (&mine["total"], &mine["truncated"]),
        (&json!(2), &json!(true))
    );
    let every = board.call("artifacts.list", &["--limit", "3"]);
    assert_eq!(
        listed(&board, &["--limit", "3"]),
        ["/artifacts/C", "/artifacts/B", "/artifacts/A"]
    );
    assert_eq!(
        (&every["total"], &every["truncated"]),
        (&json!(3), &json!(false))
    );
}

#[test]
fn list_keeps_one_state_one_area_or_the_unfiled_artifacts() {
    let board = board();
    let filed = json!({"subject": ["/changes/a"], "shows": "The step list.", "state": "shown", "area": ["dashboard/flows"]});
    board
        .write("changes/a.json", r#"{"$type": "changes/change/v1"}"#)
        .write("artifacts/A.json", &artifact(A, &filed))
        .write(
            "artifacts/B.json",
            &artifact(B, &json!({"subject": ["/changes/a"]})),
        );
    assert_eq!(listed(&board, &["--state", "shown"]), ["/artifacts/A"]);
    assert_eq!(listed(&board, &["--state", "draft"]), ["/artifacts/B"]);
    assert_eq!(
        listed(&board, &["--area", "dashboard/flows"]),
        ["/artifacts/A"]
    );
    assert_eq!(
        listed(&board, &["--area", "dashboard"]),
        Vec::<String>::new()
    );
    assert_eq!(listed(&board, &["--unfiled", "true"]), ["/artifacts/B"]);
    assert_eq!(
        listed(&board, &["--unfiled", "false"]),
        ["/artifacts/B", "/artifacts/A"]
    );
}

#[test]
fn list_keeps_the_artifacts_whose_subject_reaches_the_path_through_a_link() {
    let board = board();
    board
        .write("changes/a.json", r#"{"$type": "changes/change/v1"}"#)
        .write("changes/b.json", r#"{"$type": "changes/change/v1"}"#)
        .write(
            "work/a.json",
            r#"{"$type": "board/mount/v1", "path": "/changes/a"}"#,
        )
        .write(
            "artifacts/A.json",
            &artifact(A, &json!({"subject": ["/changes/a"]})),
        )
        .write(
            "artifacts/B.json",
            &artifact(B, &json!({"subject": ["/changes/b"]})),
        );
    assert_eq!(
        listed(&board, &["--subject", "/changes/a"]),
        ["/artifacts/A"]
    );
    assert_eq!(listed(&board, &["--subject", "/work/a"]), ["/artifacts/A"]);
    assert_eq!(
        listed(&board, &["--subject", "/changes/b"]),
        ["/artifacts/B"]
    );
}

#[test]
fn a_record_board_marks_degraded_is_counted_and_left_out_and_a_valid_one_is_listed() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({})))
        .write(
            "artifacts/A/versions/1.json",
            &version("2026-10-31T18:20:00Z", &json!({"version": "1"})),
        )
        .write(
            "artifacts/A/versions/2.json",
            &version(
                "2026-11-01T09:00:00Z",
                &json!({"version": "2", "sha256": "bad"}),
            ),
        );
    let output = board.call("artifacts.list", &[]);
    assert_eq!(
        (
            &output["artifacts"][0]["versions"][0]["version"],
            &output["unreadable"]
        ),
        (&json!("1"), &json!(1))
    );
    board.write(
        "artifacts/A/versions/2.json",
        &version("2026-11-01T09:00:00Z", &json!({"version": "2"})),
    );
    let output = board.call("artifacts.list", &[]);
    assert_eq!(
        (
            &output["artifacts"][0]["versions"][0]["version"],
            &output["unreadable"]
        ),
        (&json!("2"), &json!(0))
    );
}

#[test]
fn list_reads_artifacts_in_a_mounted_board() {
    let board = board();
    let (repository, commit) = repository(
        &["artifacts"],
        &[
            ("artifacts/A.json", artifact(A, &json!({}))),
            (
                "artifacts/A/versions/1.json",
                version("2026-10-31T18:20:00Z", &json!({"version": "1"})),
            ),
        ],
    );
    mount(&board, "boards/other.json", &repository, commit);
    let output = board.call("artifacts.list", &[]);
    let entry = &output["artifacts"][0];
    assert_eq!(
        (&entry["path"], &entry["versions"][0]["path"]),
        (
            &json!("/boards/other/artifacts/A"),
            &json!("/boards/other/artifacts/A/versions/1")
        )
    );
}

#[test]
fn list_answers_the_same_from_a_cold_index_as_from_a_warm_one_after_a_change() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({})))
        .write(
            "artifacts/A/versions/1.json",
            &version("2026-10-31T18:20:00Z", &json!({"version": "1"})),
        );
    let before = board.answer("artifacts.list", &[], false);
    board.write("artifacts/B.json", &artifact(B, &json!({})));
    let warm = board.answer("artifacts.list", &[], false);
    assert_ne!(warm, before, "the change is seen");
    assert_eq!(warm, board.answer("artifacts.list", &[], true));
}

/// An Artifact tool list result with a row for each of `urls`.
fn listing(urls: &[&str], fields: &Value) -> Value {
    let rows: Vec<Value> = (urls.iter())
        .map(|url| json!({"title": "Page", "url": url, "updatedAt": "2026-11-01T02:41:45Z", "rel": "mine"}))
        .collect();
    let mut listing = json!({"artifacts": rows, "pins_enabled": true});
    for (name, value) in fields.as_object().unwrap() {
        listing[name] = value.clone();
    }
    listing
}

#[test]
fn import_writes_only_the_artifacts_the_board_does_not_have() {
    let board = board();
    let held = artifact(A, &json!({"first_seen": "2026-10-01T09:00:00Z"}));
    board.write("artifacts/A.json", &held);
    let input = json!({"account": ACCOUNT, "listing": listing(&[A, B, C], &json!({}))});
    let output = given(&board, "import", &input);
    assert_eq!(
        output,
        json!({
            "written": ["/artifacts/B", "/artifacts/0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21"],
            "known": 1,
            "truncated": null,
        })
    );
    assert_eq!(board.read("artifacts/A.json"), held);
    assert_eq!(
        stored(&board, "artifacts/B.json"),
        json!({
            "$type": "artifacts/artifact/v1",
            "url": B,
            "account": ACCOUNT,
            "first_seen": "2026-11-01T02:41:45Z",
            "state": "draft",
        })
    );
    let again = given(&board, "import", &input);
    assert_eq!(
        (&again["written"], &again["known"]),
        (&json!([]), &json!(3))
    );
}

#[test]
fn a_row_without_its_update_time_is_first_seen_at_the_import() {
    let board = board();
    let mut rows = listing(&[A], &json!({}));
    rows["artifacts"][0]
        .as_object_mut()
        .unwrap()
        .remove("updatedAt");
    given(
        &board,
        "import",
        &json!({"account": ACCOUNT, "listing": rows}),
    );
    let first_seen = stored(&board, "artifacts/A.json")["first_seen"].clone();
    let first_seen = first_seen.as_str().unwrap();
    assert!(
        first_seen.ends_with('Z') && first_seen.len() == 20,
        "{first_seen}"
    );
    assert!(first_seen > "2026-01-01", "{first_seen}");
}

#[test]
fn a_truncated_listing_is_reported_with_its_total_and_a_whole_one_is_not() {
    let board = board();
    let cut = listing(&[A, B], &json!({"truncated": true, "total": 238}));
    let output = given(
        &board,
        "import",
        &json!({"account": ACCOUNT, "listing": cut}),
    );
    assert_eq!(
        output["truncated"],
        json!({
            "listed": 2,
            "total": 238,
            "message": "the listing holds the 2 most recently updated of 238 artifacts of \
                owner@example.com, and the Artifact tool lists no more than 200, so the rest are \
                not imported; a regular sync catches each one when it is updated",
        })
    );
    let whole = listing(&[A, B, C], &json!({"truncated": false}));
    let output = given(
        &board,
        "import",
        &json!({"account": ACCOUNT, "listing": whole}),
    );
    assert_eq!(output["truncated"], Value::Null);
}

#[test]
fn a_listing_that_is_not_the_artifact_tools_list_result_is_refused_and_one_that_is_is_admitted() {
    let board = board();
    let bad_input = "board::plugin::bad_input";
    let rows = listing(&[A], &json!({}))["artifacts"].clone();
    let items = json!({"account": ACCOUNT, "listing": {"items": rows}});
    assert_eq!(refused_given(&board, "import", &items), bad_input);
    let mut untitled = listing(&[A], &json!({}));
    untitled["artifacts"][0]
        .as_object_mut()
        .unwrap()
        .remove("title");
    let untitled = json!({"account": ACCOUNT, "listing": untitled});
    assert_eq!(refused_given(&board, "import", &untitled), bad_input);
    let elsewhere =
        json!({"account": ACCOUNT, "listing": listing(&["https://example.com/A"], &json!({}))});
    assert_eq!(refused_given(&board, "import", &elsewhere), bad_input);
    let admitted = json!({"account": ACCOUNT, "listing": listing(&[A], &json!({}))});
    assert_eq!(
        given(&board, "import", &admitted)["written"],
        json!(["/artifacts/A"])
    );
}

#[test]
fn a_listing_of_shared_artifacts_is_refused_and_a_listing_of_the_accounts_own_is_admitted() {
    let board = board();
    for scope in ["shared", "all"] {
        let input = json!({"account": ACCOUNT, "listing": listing(&[A], &json!({"scope": scope}))});
        assert_eq!(
            refused_given(&board, "import", &input),
            "board::plugin::bad_input"
        );
    }
    let mine = json!({"account": ACCOUNT, "listing": listing(&[A], &json!({"scope": "mine"}))});
    assert_eq!(
        given(&board, "import", &mine)["written"],
        json!(["/artifacts/A"])
    );
}

#[test]
fn an_import_without_an_account_is_refused_and_one_with_it_is_admitted() {
    let board = board();
    let rows = listing(&[A], &json!({}));
    for input in [
        json!({"account": "", "listing": rows}),
        json!({"listing": rows}),
    ] {
        assert_eq!(
            refused_given(&board, "import", &input),
            "board::plugin::bad_input"
        );
    }
    let admitted = json!({"account": ACCOUNT, "listing": rows});
    assert_eq!(
        given(&board, "import", &admitted)["written"],
        json!(["/artifacts/A"])
    );
}

#[test]
fn record_writes_a_new_artifact_in_draft_with_its_subject_once() {
    let board = board();
    board.write("changes/a.json", r#"{"$type": "changes/change/v1"}"#);
    let input = json!({"url": A, "account": ACCOUNT, "subject": ["/changes/a"]});
    let output = given(&board, "record", &input);
    assert_eq!(
        output,
        json!({
            "path": "/artifacts/A",
            "created": true,
            "subject": ["/changes/a"],
            "shows": null,
            "state": "draft",
            "may_replace": [],
        })
    );
    let written = stored(&board, "artifacts/A.json");
    assert_eq!(
        (&written["subject"], &written["state"]),
        (&json!(["/changes/a"]), &json!("draft"))
    );
    let before = board.read("artifacts/A.json");
    let again = given(
        &board,
        "record",
        &json!({"url": A, "account": ACCOUNT, "subject": ["/changes/b"]}),
    );
    assert_eq!(
        (&again["created"], &again["subject"]),
        (&json!(false), &json!(["/changes/a"]))
    );
    assert_eq!(board.read("artifacts/A.json"), before);
}

#[test]
fn record_gives_a_held_artifact_without_a_subject_the_one_the_session_names() {
    let board = board();
    board.write(
        "artifacts/A.json",
        &artifact(A, &json!({"shows": "The plan."})),
    );
    let output = given(
        &board,
        "record",
        &json!({"url": A, "account": ACCOUNT, "subject": ["/changes/a"]}),
    );
    assert_eq!(
        (&output["created"], &output["subject"], &output["shows"]),
        (&json!(false), &json!(["/changes/a"]), &json!("The plan."))
    );
    assert_eq!(
        stored(&board, "artifacts/A.json")["subject"],
        json!(["/changes/a"])
    );
}

#[test]
fn record_names_the_shown_or_agreed_artifacts_of_the_same_subject_and_not_a_draft_one() {
    let board = board();
    board
        .write("changes/a.json", r#"{"$type": "changes/change/v1"}"#)
        .write(
            "artifacts/A.json",
            &artifact(A, &json!({"subject": ["/changes/a"], "state": "shown"})),
        )
        .write(
            "artifacts/B.json",
            &artifact(B, &json!({"subject": ["/changes/a"]})),
        );
    let output = given(
        &board,
        "record",
        &json!({"url": C, "account": ACCOUNT, "subject": ["/changes/a"]}),
    );
    assert_eq!(output["may_replace"], json!(["/artifacts/A"]));
    let unrelated = "https://claude.ai/artifact/D";
    let output = given(
        &board,
        "record",
        &json!({"url": unrelated, "account": ACCOUNT}),
    );
    assert_eq!(output["may_replace"], json!([]));
}

#[test]
fn set_adds_a_subject_and_area_replaces_shows_and_sets_shown() {
    let board = board();
    board.write(
        "artifacts/A.json",
        &artifact(A, &json!({"subject": ["/changes/a"]})),
    );
    let output = board.call(
        "artifacts.set",
        &[
            "--path",
            "/artifacts/A",
            "--subject",
            "/changes/b",
            "--area",
            "dashboard/flows",
            "--shows",
            "The step list.",
            "--state",
            "shown",
        ],
    );
    assert_eq!(
        output,
        json!({
            "path": "/artifacts/A",
            "subject": ["/changes/a", "/changes/b"],
            "shows": "The step list.",
            "state": "shown",
            "area": ["dashboard/flows"],
            "superseded": null,
        })
    );
    let again = board.call(
        "artifacts.set",
        &["--path", "/artifacts/A", "--subject", "/changes/b"],
    );
    assert_eq!(again["subject"], json!(["/changes/a", "/changes/b"]));
}

#[test]
fn set_with_nothing_to_change_is_refused_and_with_one_field_is_admitted() {
    let board = board();
    board.write("artifacts/A.json", &artifact(A, &json!({})));
    let failure = board.refusal("artifacts.set", &["--path", "/artifacts/A"]);
    assert_eq!(failure["code"], "artifacts::set::nothing");
    let output = board.call(
        "artifacts.set",
        &["--path", "/artifacts/A", "--shows", "The plan."],
    );
    assert_eq!(output["shows"], "The plan.");
}

#[test]
fn set_on_a_path_that_holds_no_artifact_is_refused_and_on_an_artifact_is_admitted() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({})))
        .write("notes/a.json", r#"{"$type": "changes/change/v1"}"#);
    for path in ["/artifacts/Z", "/notes/a"] {
        let failure = board.refusal("artifacts.set", &["--path", path, "--shows", "The plan."]);
        assert_eq!(failure["code"], "artifacts::artifact::not_found", "{path}");
    }
    board.call(
        "artifacts.set",
        &["--path", "/artifacts/A", "--shows", "The plan."],
    );
}

#[test]
fn set_supersedes_the_old_artifact_in_the_same_commit() {
    let board = board();
    board
        .write("artifacts/A.json", &artifact(A, &json!({"state": "shown"})))
        .write("artifacts/B.json", &artifact(B, &json!({})));
    let output = board.call(
        "artifacts.set",
        &["--path", "/artifacts/B", "--supersedes", "/artifacts/A"],
    );
    assert_eq!(output["superseded"], "/artifacts/A");
    let old = stored(&board, "artifacts/A.json");
    assert_eq!(
        (&old["state"], &old["superseded_by"]),
        (&json!("superseded"), &json!("/artifacts/B"))
    );
}

#[test]
fn set_refuses_a_state_on_an_agreed_artifact_and_admits_it_on_a_shown_one() {
    let board = board();
    let agreed = json!({"state": "agreed", "agreed_by": "/projects/a/decisions/d"});
    board
        .write("artifacts/A.json", &artifact(A, &agreed))
        .write("artifacts/B.json", &artifact(B, &json!({"state": "shown"})));
    let failure = board.refusal(
        "artifacts.set",
        &["--path", "/artifacts/A", "--state", "draft"],
    );
    assert_eq!(failure["code"], "artifacts::state::settled");
    let output = board.call(
        "artifacts.set",
        &["--path", "/artifacts/B", "--state", "draft"],
    );
    assert_eq!(output["state"], "draft");
    assert_eq!(
        board.refusal(
            "artifacts.set",
            &["--path", "/artifacts/B", "--state", "agreed"]
        )["code"],
        "board::plugin::bad_input"
    );
}

/// A board that enables artifacts and plans, with an artifact, an answer
/// of the owner's, and a decision that names it.
fn decided() -> Fixture {
    let board = Fixture::enabling(&["artifacts", "plans"]);
    let answer = json!({
        "$type": "plans/answer/v1",
        "choice": "none",
        "words": "Yes, that step list is what I want.",
        "answered": "2026-11-01T09:00:00Z",
    });
    let decision = |refs: Value| {
        json!({
            "$type": "plans/decision/v1",
            "decision": "The step list is agreed.",
            "why": "The owner said so.",
            "session": "local_0a1b",
            "decided": "2026-11-01T09:05:00Z",
            "refs": refs,
        })
        .to_string()
    };
    board
        .write("artifacts/A.json", &artifact(A, &json!({"state": "shown"})))
        .write("projects/a/answers/x.json", &answer.to_string())
        .write(
            "projects/a/decisions/owners.json",
            &decision(json!(["/projects/a/answers/x", "/artifacts/A"])),
        )
        .write(
            "projects/a/decisions/agents.json",
            &decision(json!(["/artifacts/A"])),
        );
    board
}

#[test]
fn agree_moves_an_artifact_to_agreed_with_a_decision_that_names_the_owners_answer() {
    let board = decided();
    let output = board.call(
        "artifacts.agree",
        &[
            "--path",
            "/artifacts/A",
            "--decision",
            "/projects/a/decisions/owners",
        ],
    );
    assert_eq!(
        output,
        json!({"path": "/artifacts/A", "state": "agreed", "agreed_by": "/projects/a/decisions/owners"})
    );
    let held = stored(&board, "artifacts/A.json");
    assert_eq!(
        (&held["state"], &held["agreed_by"]),
        (&json!("agreed"), &json!("/projects/a/decisions/owners"))
    );
}

#[test]
fn agree_refuses_a_decision_that_is_missing_or_not_the_owners() {
    let board = decided();
    let stub = json!({
        "$type": "plans/decision/v1",
        "decision": "Opened a pull request.",
        "why": "",
        "session": "local_0a1b",
        "decided": "2026-11-01T09:05:00Z",
        "refs": ["/projects/a/answers/x"],
        "written_by": "runs/pr-create",
    });
    board.write("projects/a/decisions/stub.json", &stub.to_string());
    for decision in [
        "/projects/a/decisions/none",
        "/projects/a/answers/x",
        "/projects/a/decisions/agents",
        "/projects/a/decisions/stub",
    ] {
        let failure = board.refusal(
            "artifacts.agree",
            &["--path", "/artifacts/A", "--decision", decision],
        );
        assert_eq!(
            failure["code"], "artifacts::agree::no_decision",
            "{decision}"
        );
        assert_eq!(failure["fields"]["decision"], decision);
    }
    assert_eq!(stored(&board, "artifacts/A.json")["state"], "shown");
    board.call(
        "artifacts.agree",
        &[
            "--path",
            "/artifacts/A",
            "--decision",
            "/projects/a/decisions/owners",
        ],
    );
}
