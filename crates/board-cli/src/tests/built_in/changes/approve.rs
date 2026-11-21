//! The owner's answer from the questions pane passes the owner-approval
//! check, and so does the owner's Touch ID approval of `changes.approve`
//! for the current plan, which the test signer signs in place of Touch ID.

use super::*;

const QUESTION: &str = "/projects/a/questions/q";

/// The owner's personal board, enabling plans and changes, holding the
/// change /changes/t in plan with its plan, and an open question that
/// blocks it whose option `a` says `approve`. The question is attached to
/// the change, as the asking session attaches it.
fn asked(approve: &str) -> Personal {
    let personal = Personal::enabling(&["plans", "changes"]);
    let change = json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/", "phase": "plan",
        "route": "owner", "plan": "/changes/t/plans/1",
    });
    let plan = json!({
        "$type": "changes/plan/v1", "text": "/changes/t/plans/1.md", "uses": [],
        "answers": answers(),
    });
    let question = json!({
        "$type": "plans/question/v1", "question": "Approve?", "by": {"session": "local_0a1b"},
        "asked": "2026-11-20T09:00:00Z", "status": "open", "blocks": "/changes/t",
        "options": [{"id": "a", "text": approve}, {"id": "b", "text": "Hold the work."}],
    });
    personal
        .write("changes/t.json", &change.to_string())
        .write("changes/t/plans/1.json", &plan.to_string())
        .write("changes/t/plans/1.md", PLAN)
        .write("projects/a/questions/q.json", &question.to_string());
    let attach = [
        "call",
        "changes.attach",
        "--path",
        "/changes/t",
        "--ref",
        QUESTION,
    ];
    let attached = personal.run(&attach);
    assert_eq!(attached.code, 0, "{}", text(&attached.stderr));
    personal
}

/// `board --json call <capability>` with `flags` in the personal board: its
/// exit code and its one document.
fn called(personal: &Personal, capability: &str, flags: &[&str]) -> (u8, Value) {
    let arguments: Vec<&str> = ["--json", "call", capability]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let ran = personal.run(&arguments);
    (ran.code, json(&ran))
}

/// Answers the question with option `a`, with `flags` added.
fn answered(personal: &Personal, flags: &[&str]) -> Value {
    let mut given = vec!["--path", QUESTION, "--option", "a"];
    given.extend(flags);
    let (code, document) = called(personal, "plans.answer", &given);
    assert_eq!(code, 0, "{document}");
    document["data"]["output"].clone()
}

/// The status and detail of the change's owner-approval check.
fn approval(personal: &Personal) -> (String, String) {
    let (code, document) = called(personal, "changes.next", &["--path", "/changes/t"]);
    assert_eq!(code, 0, "{document}");
    let gate = document["data"]["output"]["gate"]
        .as_array()
        .unwrap()
        .clone();
    let check = gate
        .iter()
        .find(|each| each["check"] == "owner-approval")
        .unwrap();
    let field = |name: &str| check[name].as_str().unwrap().to_owned();
    (field("status"), field("detail"))
}

#[test]
fn the_panes_answer_to_an_option_naming_the_change_passes_owner_approval_and_its_twin_does_not() {
    let personal = asked("Approve the plan at /changes/t.");
    let output = answered(&personal, &["--written_by", "plans/pane-answer"]);
    assert_eq!(output["attached"], json!({"change": "/changes/t"}));
    assert_eq!(approval(&personal).0, "pass");
    let twin = asked("Approve the plan.");
    answered(&twin, &["--written_by", "plans/pane-answer"]);
    assert_eq!(approval(&twin).0, "missing");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "it signs the approval with a key the allowed signers file does not list"
)]
fn approve_asks_the_owner_and_owner_approval_passes_once_the_owner_signs_it() {
    let personal = asked("Approve the plan at /changes/t.");
    let refused = called(&personal, "changes.approve", &["--path", "/changes/t"]).1;
    assert_eq!(refused["code"], "changes::approve::not_chosen", "{refused}");
    assert_eq!(personal.pending(), Vec::<String>::new());
    // An answer that names neither a person nor a surface, which the
    // unsigned route does not count.
    answered(&personal, &[]);
    let (code, asked) = called(&personal, "changes.approve", &["--path", "/changes/t"]);
    assert_eq!(code, 0, "{asked}");
    let output = &asked["data"]["output"];
    assert_eq!(output["state"], "asked");
    assert_eq!(output["plan"], "/changes/t/plans/1");
    let effect = output["effect"].as_str().unwrap().to_owned();
    assert_eq!(personal.pending(), std::slice::from_ref(&effect));
    assert_eq!(approval(&personal).0, "missing");
    personal.answer(&effect, "approve");
    assert_eq!(approval(&personal), ("pass".to_owned(), String::new()));
    let again = called(&personal, "changes.approve", &["--path", "/changes/t"]).1;
    assert_eq!(again["data"]["output"]["state"], "approved");
    // The twin: the same approval signed by a key the allowed signers file
    // does not list.
    let held = personal
        .record(&format!("/effects/answered/{effect}"))
        .unwrap();
    let pending: board_model::Effect = serde_json::from_value(held).unwrap();
    let seen_at = format!("effects/seen/{effect}.json");
    let mut seen = personal.record(&format!("/effects/seen/{effect}")).unwrap();
    seen["signature"] = json!(Signer::new().sign(&pending.approval()));
    personal.write(&seen_at, &seen.to_string());
    let (status, detail) = approval(&personal);
    assert_eq!(status, "missing");
    assert!(
        detail.contains("; Board could not read that approval: "),
        "{detail}"
    );
}
