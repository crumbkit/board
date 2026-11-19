//! `sweep` withdraws each open question that no longer needs the owner, with
//! a reason, and keeps its twin that still does.

use std::rc::Rc;

use board_ops::{Board, Plugins};
use board_plugin::Target;
use board_scenario::Item;
use serde_json::{Value, json};

use crate::open::tests::{
    MOUNTED_QUESTION, answer_naming, body, change, host, mounting_board, plans_board,
};

const ASKED: &str = "2026-10-01T09:00:00Z";

/// What `plans.sweep` returns for `input` on `board`.
fn sweep(board: &Board, input: &Value) -> Value {
    let target: Target = "plans.sweep".parse().unwrap();
    let input = input.to_string();
    board
        .call(&target, input.as_bytes(), &Plugins::default())
        .unwrap()
        .output
}

/// The paths `plans.open` lists on `board`, sorted.
fn open(board: &Board) -> Vec<String> {
    let target: Target = "plans.open".parse().unwrap();
    let output = board
        .call(&target, b"{}", &Plugins::default())
        .unwrap()
        .output;
    let mut paths: Vec<String> = (output["questions"].as_array().unwrap().iter())
        .map(|question| question["path"].as_str().unwrap().to_owned())
        .collect();
    paths.sort();
    paths
}

/// Each withdrawn question's path and kind, sorted by path.
fn withdrawn(output: &Value) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = (output["withdrawn"].as_array().unwrap().iter())
        .map(|each| {
            let text = |name: &str| each[name].as_str().unwrap().to_owned();
            (text("path"), text("kind"))
        })
        .collect();
    found.sort();
    found
}

/// A question asked by `session` on `day` of October with `fields` added.
fn asked_by(session: &str, day: u8, fields: &Value) -> Value {
    let mut fields = fields.clone();
    fields["by"] = json!({"session": session});
    body(&format!("2026-10-{day:02}T09:00:00Z"), &fields)
}

#[test]
fn a_question_blocking_ended_work_is_withdrawn_and_its_twin_blocking_active_work_stays() {
    let (_scenario, board) = plans_board(&[
        ("changes/done", change("done")),
        ("changes/dropped", change("dropped")),
        ("changes/going", change("build")),
        (
            "projects/a/questions/done",
            body(ASKED, &json!({"blocks": "/changes/done"})),
        ),
        (
            "projects/a/questions/dropped",
            body(ASKED, &json!({"blocks": "/changes/dropped"})),
        ),
        (
            "projects/a/questions/going",
            body(ASKED, &json!({"blocks": "/changes/going"})),
        ),
    ]);
    let output = sweep(&board, &json!({"apply": true}));
    assert_eq!(
        withdrawn(&output),
        [
            ("/projects/a/questions/done".to_owned(), "done".to_owned()),
            (
                "/projects/a/questions/dropped".to_owned(),
                "dropped".to_owned()
            ),
        ]
    );
    let reasons: Vec<&str> = (output["withdrawn"].as_array().unwrap().iter())
        .map(|each| each["reason"].as_str().unwrap())
        .collect();
    assert!(reasons.contains(&"plans.sweep: the work it blocks, /changes/done, is done."));
    assert_eq!(open(&board), ["/projects/a/questions/going"]);
    assert_eq!(output["remaining"], json!(1));
}

#[test]
fn a_question_from_an_archived_session_is_withdrawn_and_its_twin_from_a_live_one_stays() {
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/gone",
            asked_by("local_dead0", 1, &json!({"question": "One?"})),
        ),
        (
            "projects/a/questions/live",
            asked_by("local_a11e", 1, &json!({"question": "Two?"})),
        ),
    ]);
    let output = sweep(&board, &json!({"apply": true, "archived": ["local_dead0"]}));
    assert_eq!(
        withdrawn(&output),
        [(
            "/projects/a/questions/gone".to_owned(),
            "archived".to_owned()
        )]
    );
    assert_eq!(
        output["withdrawn"][0]["reason"],
        json!("plans.sweep: the session that asked it, local_dead0, is archived.")
    );
    assert_eq!(open(&board), ["/projects/a/questions/live"]);
}

/// Two open questions in one project with the same words both stay, because
/// `plans.ask` merges repeats when they are filed and the sweep leaves them.
#[test]
fn two_open_questions_with_the_same_words_both_stay() {
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/first",
            asked_by("local_aa", 1, &json!({"question": "Which way, north?"})),
        ),
        (
            "projects/a/questions/again",
            asked_by("local_bb", 2, &json!({"question": "which way  north"})),
        ),
    ]);
    let output = sweep(&board, &json!({"apply": true}));
    assert_eq!(output["total"], json!(0));
    assert_eq!(
        open(&board),
        ["/projects/a/questions/again", "/projects/a/questions/first"]
    );
}

/// An open question whose words match a question already answered in the
/// same project is withdrawn and names that answer. Its twin in another
/// project stays.
#[test]
fn a_question_already_answered_in_its_project_is_withdrawn_and_its_twin_elsewhere_stays() {
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/before",
            asked_by("local_aa", 1, &json!({"question": "Ship it?"})),
        ),
        (
            "projects/a/answers/before",
            answer_naming("/projects/a/questions/before"),
        ),
        (
            "projects/a/questions/again",
            asked_by("local_bb", 2, &json!({"question": "Ship it?"})),
        ),
        (
            "projects/b/questions/again",
            asked_by("local_bb", 2, &json!({"question": "Ship it?"})),
        ),
    ]);
    let output = sweep(&board, &json!({"apply": true}));
    assert_eq!(
        withdrawn(&output),
        [(
            "/projects/a/questions/again".to_owned(),
            "answered".to_owned()
        )]
    );
    assert_eq!(
        output["withdrawn"][0]["reason"],
        json!(
            "plans.sweep: /projects/a/questions/before asked the same and was answered at /projects/a/answers/before."
        )
    );
    assert_eq!(
        output["counts"],
        json!({"done": 0, "dropped": 0, "archived": 0, "answered": 1})
    );
    assert_eq!(open(&board), ["/projects/b/questions/again"]);
}

/// Without `apply` the sweep lists what it would withdraw and writes
/// nothing. With it, each question keeps its record, now withdrawn with its
/// reason.
#[test]
fn without_apply_nothing_is_written_and_with_it_each_question_is_kept_withdrawn() {
    let (scenario, board) = plans_board(&[
        ("changes/done", change("done")),
        (
            "projects/a/questions/q",
            body(ASKED, &json!({"blocks": "/changes/done"})),
        ),
    ]);
    let output = sweep(&board, &json!({}));
    assert_eq!(output["applied"], json!(false));
    assert_eq!(output["total"], json!(1));
    assert_eq!(open(&board), ["/projects/a/questions/q"]);
    let output = sweep(&board, &json!({"apply": true}));
    assert_eq!(output["applied"], json!(true));
    assert_eq!(open(&board), Vec::<String>::new());
    let text =
        std::fs::read_to_string(scenario.workdir().join("projects/a/questions/q.json")).unwrap();
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (&record["status"], &record["withdrawn_reason"]),
        (
            &json!("withdrawn"),
            &json!("plans.sweep: the work it blocks, /changes/done, is done.")
        )
    );
}

/// A path sweeps only the questions below it.
#[test]
fn a_path_sweeps_only_the_questions_below_it() {
    let (_scenario, board) = plans_board(&[
        (
            "projects/a/questions/q",
            asked_by("local_dead0", 1, &json!({})),
        ),
        (
            "projects/b/questions/q",
            asked_by("local_dead0", 1, &json!({})),
        ),
    ]);
    let output = sweep(
        &board,
        &json!({"apply": true, "archived": ["local_dead0"], "path": "/projects/a"}),
    );
    assert_eq!(
        withdrawn(&output),
        [("/projects/a/questions/q".to_owned(), "archived".to_owned())]
    );
    assert_eq!(open(&board), ["/projects/b/questions/q"]);
}

/// Board refuses an archived session that is not a `local_` id with its
/// input code, and admits the twin that is one.
#[test]
fn board_refuses_an_archived_session_that_is_no_local_id_and_admits_its_twin() {
    let (_scenario, board) = plans_board(&[]);
    let target: Target = "plans.sweep".parse().unwrap();
    let refused = board
        .call(&target, br#"{"archived": ["gone"]}"#, &Plugins::default())
        .unwrap_err();
    assert_eq!(refused.refusal().code.as_str(), "board::plugin::bad_input");
    assert!(
        board
            .call(
                &target,
                br#"{"archived": ["local_dead0"]}"#,
                &Plugins::default()
            )
            .is_ok()
    );
}

/// A put batch may not cross a mount, so the sweep writes the questions of
/// each board in their own batch. A dead question in the root board and one
/// in a mounted board are both withdrawn. The library gets one commit. The
/// root gets two: its own batch, then the move of the mount's pin to the
/// library's new commit, which Board makes after every write through a
/// mount (docs/architecture/writes.md). Each carries the sweep's trailer.
#[test]
fn questions_in_the_root_and_a_mounted_board_are_both_withdrawn() {
    let dead = asked_by("local_dead0", 1, &json!({}));
    let (root, library, board) = mounting_board(
        &[("projects/a/questions/root", dead.clone())],
        &[(MOUNTED_QUESTION, dead)],
    );
    assert!(
        root.head().is_none(),
        "the root has no commit before the sweep"
    );
    let pinned = library.head().unwrap().id;
    let output = sweep(&board, &json!({"apply": true, "archived": ["local_dead0"]}));
    assert_eq!(output["total"], json!(2));
    assert_eq!(open(&board), Vec::<String>::new());
    let library_head = library.head().unwrap();
    assert_eq!(library_head.parents, [pinned]);
    let mounted: Value =
        serde_json::from_slice(&library_head.files[&format!("{MOUNTED_QUESTION}.json")]).unwrap();
    assert_eq!(mounted["status"], json!("withdrawn"));
    let subjects: Vec<String> = root
        .history()
        .into_iter()
        .map(|(_, subject)| subject)
        .collect();
    assert_eq!(subjects, ["board put --batch (1 records)"; 2]);
    let root_head = root.head().unwrap();
    let mount: Value = serde_json::from_slice(&root_head.files["vendor.json"]).unwrap();
    assert_eq!(mount["commit"], json!(library_head.id.to_string()));
    for head in [root_head, library_head] {
        assert_eq!(
            head.message,
            "board put --batch (1 records)\n\nBoard-Bulk: sweep questions"
        );
    }
}

/// A question answered or withdrawn after the sweep found it, and before it
/// read the question again to write, is refused with the sweep's own code and
/// a hint to sweep again, and no question is withdrawn. The twin, unchanged,
/// withdraws both questions.
#[test]
fn a_question_answered_or_withdrawn_before_the_write_is_refused_and_nothing_is_written() {
    let withdrawn_since = asked_by(
        "local_dead0",
        1,
        &json!({"status": "withdrawn", "withdrawn_reason": "Someone withdrew it."}),
    );
    let changes = [
        (
            Some((
                "projects/a/answers/a",
                answer_naming("/projects/a/questions/a"),
            )),
            Some("plans::sweep::already_answered"),
        ),
        (
            Some(("projects/a/questions/a", withdrawn_since)),
            Some("plans::sweep::not_open"),
        ),
        (None, None),
    ];
    for (change, code) in changes {
        let dead = asked_by("local_dead0", 1, &json!({}));
        let (scenario, board) = plans_board(&[
            ("projects/a/questions/a", dead.clone()),
            ("projects/a/questions/b", dead),
        ]);
        let mut host = host(Vec::new());
        host.board = Some(board);
        let scenario = Rc::new(scenario);
        if let Some((path, record)) = change {
            let there = Rc::clone(&scenario);
            *host.before_read.borrow_mut() = Some(Box::new(move || {
                there.write(
                    &format!("{path}.json"),
                    Item::File(record.as_str().unwrap()),
                );
            }));
        }
        let result =
            crate::sweep::sweep(&host, &json!({"apply": true, "archived": ["local_dead0"]}));
        let Some(code) = code else {
            assert_eq!(result.unwrap()["total"], json!(2));
            assert_eq!(open(host.board.as_ref().unwrap()), Vec::<String>::new());
            continue;
        };
        let refused = result.unwrap_err();
        assert_eq!(refused.code, code);
        assert_eq!(
            refused.hint.as_deref(),
            Some("run `board plans sweep` again; it reads each question afresh")
        );
        assert!(host.puts.borrow().is_empty(), "{code} puts nothing");
        let b = std::fs::read_to_string(scenario.workdir().join("projects/a/questions/b.json"));
        let b: Value = serde_json::from_str(&b.unwrap()).unwrap();
        assert_eq!(b["status"], json!("open"), "{code} leaves b open");
    }
}
