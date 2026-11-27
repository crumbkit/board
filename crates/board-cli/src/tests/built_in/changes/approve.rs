//! The owner's answer from the questions pane passes the owner-approval
//! check, and so does the owner's Touch ID approval of `changes.approve`
//! for the current plan, which the test signer signs in place of Touch ID.

use super::*;

const QUESTION: &str = "/projects/a/questions/q";
/// Where `plans.answer` records the answer to [`QUESTION`].
const ANSWER: &str = "/projects/a/answers/q";

/// The owner's personal board, enabling plans and changes, holding the
/// change /changes/t in plan with its plan, which stores data so it needs
/// the owner's approval, and an open question that
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
        "answers": stores(),
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

/// The typed answers of a plan that stores data, which needs the owner's
/// approval.
fn stores() -> Value {
    json!({"ux": "none", "data": "stored", "lifecycle": "none"})
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
    assert_eq!(output["answer"], ANSWER);
    let effect = output["effect"].as_str().unwrap().to_owned();
    assert_eq!(personal.pending(), std::slice::from_ref(&effect));
    // The Touch ID statement shows the plan's title and digest, the answer
    // and the text of the option the owner chose.
    let pending = personal
        .record(&format!("/effects/pending/{effect}"))
        .unwrap();
    let shown = &pending["record"];
    assert_eq!(
        (&shown["plan_title"], &shown["answer"], &shown["chosen"]),
        (
            &json!("Let a suite declare a timeout"),
            &json!(ANSWER),
            &json!("Approve the plan at /changes/t.")
        )
    );
    assert!(
        shown["plan_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
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

/// The fixture with an answer that chose the option naming the change, and
/// the owner's signed approval of the plan as it is: owner-approval passes.
fn signed() -> Personal {
    let personal = asked("Approve the plan at /changes/t.");
    answered(&personal, &[]);
    let asked = called(&personal, "changes.approve", &["--path", "/changes/t"]).1;
    let effect = asked["data"]["output"]["effect"]
        .as_str()
        .unwrap()
        .to_owned();
    personal.answer(&effect, "approve");
    assert_eq!(approval(&personal).0, "pass");
    personal
}

/// Writes a plan record at `/changes/t/plans/<name>` whose text is `text`.
fn plan_record(personal: &Personal, name: &str, text: &str) {
    let plan = json!({
        "$type": "changes/plan/v1", "text": text, "uses": [], "answers": stores(),
    });
    personal.write(&format!("changes/t/plans/{name}.json"), &plan.to_string());
}

/// Points the change at the plan record `plan`.
fn point_at(personal: &Personal, plan: &str) {
    let mut change = personal.record("/changes/t").unwrap();
    change["plan"] = json!(plan);
    personal.write("changes/t.json", &change.to_string());
}

#[test]
fn attaching_a_record_after_the_owner_signs_keeps_the_approval() {
    let personal = signed();
    let attach = [
        "call",
        "changes.attach",
        "--path",
        "/changes/t",
        "--ref",
        "/changes/t",
    ];
    let attached = personal.run(&attach);
    assert_eq!(attached.code, 0, "{}", text(&attached.stderr));
    assert_eq!(approval(&personal).0, "pass");
}

#[test]
fn new_plan_text_put_after_the_owner_signs_makes_the_approval_missing() {
    let personal = signed();
    let edited = format!("{PLAN}\nAnd one more thing.\n");
    let put = personal.run_in(
        personal.board.workdir(),
        &["put", "/changes/t/plans/1.md"],
        &edited,
    );
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    assert_eq!(approval(&personal).0, "missing");
}

#[test]
fn plan_text_written_on_disk_after_the_owner_signs_makes_the_approval_missing() {
    let personal = signed();
    personal.write("changes/t/plans/1.md", &format!("{PLAN}\nEdited.\n"));
    assert_eq!(approval(&personal).0, "missing");
}

#[test]
fn a_plan_record_repointed_to_other_text_after_the_owner_signs_makes_the_approval_missing() {
    let personal = signed();
    // The same words in another file: the record itself changed.
    personal.write("changes/t/plans/other.md", PLAN);
    plan_record(&personal, "1", "/changes/t/plans/other.md");
    assert_eq!(approval(&personal).0, "missing");
}

#[test]
fn a_later_revision_keeps_the_approval_until_it_changes_the_outcome_and_back_only_while_unedited() {
    let personal = signed();
    personal.write("changes/t/plans/2.md", PLAN);
    plan_record(&personal, "2", "/changes/t/plans/2.md");
    point_at(&personal, "/changes/t/plans/2");
    assert_eq!(
        approval(&personal),
        (
            "pass".to_owned(),
            "the owner approved plans/1 with Touch ID, and the current plan changes nothing that \
             needs a new approval"
                .to_owned()
        )
    );
    personal.write(
        "changes/t/plans/2.md",
        &PLAN.replace("A suite stops.", "A suite stops and retries."),
    );
    assert_eq!(approval(&personal).0, "missing");
    point_at(&personal, "/changes/t/plans/1");
    assert_eq!(approval(&personal).0, "pass");
    personal.write(
        "changes/t/plans/1.md",
        &format!("{PLAN}\nRewound and edited.\n"),
    );
    assert_eq!(approval(&personal).0, "missing");
}

/// Writes a new revision of /changes/t's plan through `changes.plan`, with
/// `text` and `paths`.
fn revise(personal: &Personal, text: &str, paths: &[&str]) {
    let input = json!({
        "path": "/changes/t", "text": text, "uses": [], "paths": paths, "answers": stores(),
    });
    let ran = personal.run_in(
        personal.board.workdir(),
        &["--json", "call", "changes.plan"],
        &input.to_string(),
    );
    assert_eq!(ran.code, 0, "{}", super::text(&ran.stderr));
}

#[test]
fn a_plan_revised_through_changes_plan_keeps_the_signed_approval_for_paths_and_loses_it_for_data() {
    let personal = signed();
    revise(&personal, PLAN, &["/src/**"]);
    let (status, detail) = approval(&personal);
    assert_eq!(status, "pass", "{detail}");
    assert_eq!(
        detail,
        "the owner approved plans/1 with Touch ID, and the current plan changes only its `paths` \
         since, which needs no new approval"
    );
    let data = PLAN.replace("## Data\n\nNone.", "## Data\n\nOne row per run.");
    revise(&personal, &data, &["/src/**"]);
    let (status, detail) = approval(&personal);
    assert_eq!(status, "missing");
    assert!(
        detail.starts_with(
            "the owner approved plans/1 with Touch ID, and the current plan changes its Data \
             section; ask the owner again"
        ),
        "{detail}"
    );
}

#[test]
fn a_plan_revised_through_changes_plan_after_the_panes_answer_makes_owner_approval_missing() {
    let personal = asked("Approve the plan at /changes/t.");
    answered(&personal, &["--written_by", "plans/pane-answer"]);
    assert_eq!(approval(&personal), ("pass".to_owned(), String::new()));
    revise(
        &personal,
        &PLAN.replace("A suite stops.", "A suite stops and retries."),
        &["/src/**"],
    );
    let (status, detail) = approval(&personal);
    assert_eq!(status, "missing");
    assert!(
        detail.starts_with(&format!(
            "the owner approved plans/1 in {ANSWER}, and the current plan changes its Outcome \
             section;"
        )),
        "{detail}"
    );
}

#[test]
fn approve_reports_a_refusal_the_owner_gave_and_owner_approval_stays_missing() {
    let personal = asked("Approve the plan at /changes/t.");
    answered(&personal, &[]);
    let asked = called(&personal, "changes.approve", &["--path", "/changes/t"]).1;
    let effect = asked["data"]["output"]["effect"]
        .as_str()
        .unwrap()
        .to_owned();
    personal.answer(&effect, "refuse");
    let (code, refused) = called(&personal, "changes.approve", &["--path", "/changes/t"]);
    assert_eq!(code, 0, "{refused}");
    assert_eq!(refused["data"]["output"]["state"], "refused");
    assert_eq!(refused["data"]["output"]["effect"], effect.as_str());
    assert_eq!(approval(&personal).0, "missing");
}

/// Decision 1314Z: `changes.ask` files one question whose approving option
/// names the change and its data understanding check, attaches it, and
/// attaches the same open question once when run again; the owner's one
/// answer passes owner-approval and data-signed.
#[test]
fn ask_files_and_attaches_one_question_whose_answer_passes_approval_and_data_sign_off() {
    let personal = asked("Approve the plan.");
    let understood = "/changes/t/understanding/data";
    let record = json!({
        "$type": "changes/understanding/v1", "change": "/changes/t", "subject": "data",
        "questions": [{"question": "Q?", "answer": "A.", "design": "A."}],
    });
    personal.write("changes/t/understanding/data.json", &record.to_string());
    let attach = [
        "call",
        "changes.attach",
        "--path",
        "/changes/t",
        "--ref",
        understood,
    ];
    assert_eq!(personal.run(&attach).code, 0);
    let input = json!({"path": "/changes/t", "project": "a", "by": {"session": "local_0a1b"}});
    let ask = |personal: &Personal| {
        let ran = personal.run_in(
            personal.board.workdir(),
            &["--json", "call", "changes.ask"],
            &input.to_string(),
        );
        let document = json(&ran);
        assert_eq!(ran.code, 0, "{document}");
        document["data"]["output"].clone()
    };
    let first = ask(&personal);
    assert_eq!(first["outcome"], "filed", "{first}");
    let approve = first["approve"].as_str().unwrap();
    assert!(approve.contains(understood), "{approve}");
    let again = ask(&personal);
    assert_eq!(again["path"], first["path"], "{again}");
    let plan = personal.record("/changes/t/plans/1").unwrap();
    let attached: Vec<&Value> = (plan["records"].as_array().unwrap().iter())
        .filter(|each| **each == first["path"])
        .collect();
    assert_eq!(attached.len(), 1, "{plan}");
    let question = first["path"].as_str().unwrap();
    let (code, document) = called(
        &personal,
        "plans.answer",
        &[
            "--path",
            question,
            "--option",
            "approve",
            "--written_by",
            "plans/pane-answer",
        ],
    );
    assert_eq!(code, 0, "{document}");
    assert_eq!(approval(&personal).0, "pass");
    let (_, next) = called(&personal, "changes.next", &["--path", "/changes/t"]);
    let gate = next["data"]["output"]["gate"].as_array().unwrap();
    let signed = gate
        .iter()
        .find(|each| each["check"] == "data-signed")
        .unwrap();
    assert_eq!(signed["status"], "pass", "{signed}");
}

/// `changes.ask` with the change /changes/t in `personal`, asked by one
/// session under the project a: its exit code and its one document.
fn asking(personal: &Personal) -> (u8, Value) {
    let input = json!({"path": "/changes/t", "project": "a", "by": {"session": "local_0a1b"}});
    let ran = personal.run_in(
        personal.board.workdir(),
        &["--json", "call", "changes.ask"],
        &input.to_string(),
    );
    (ran.code, json(&ran))
}

/// The output of [`asking`], which must succeed.
fn ask(personal: &Personal) -> Value {
    let (code, document) = asking(personal);
    assert_eq!(code, 0, "{document}");
    document["data"]["output"].clone()
}

/// Points /changes/t at plan `number`, written beside it.
fn planned(personal: &Personal, number: u32) {
    let plan = json!({
        "$type": "changes/plan/v1", "text": format!("/changes/t/plans/{number}.md"), "uses": [],
        "answers": stores(),
    });
    let change = json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/", "phase": "plan",
        "route": "owner", "plan": format!("/changes/t/plans/{number}"),
    });
    personal
        .write(&format!("changes/t/plans/{number}.json"), &plan.to_string())
        .write(&format!("changes/t/plans/{number}.md"), PLAN)
        .write("changes/t.json", &change.to_string());
}

#[test]
fn ask_files_one_question_per_plan_so_a_revised_plan_is_asked_again() {
    let personal = Personal::enabling(&["plans", "changes"]);
    let unplanned = json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/", "phase": "plan",
    });
    personal.write("changes/t.json", &unplanned.to_string());
    let refused = asking(&personal).1;
    assert_eq!(refused["code"], "changes::ask::no_plan", "{refused}");
    planned(&personal, 1);
    let first = ask(&personal);
    assert_eq!(first["outcome"], "filed", "{first}");
    for field in ["question", "approve"] {
        let text = first[field].as_str().unwrap();
        assert!(text.contains("/changes/t/plans/1"), "{field}: {text}");
    }
    let again = ask(&personal);
    assert_eq!(again["outcome"], "merged", "{again}");
    assert_eq!(again["path"], first["path"]);
    let asked = first["path"].as_str().unwrap();
    let (code, document) = called(
        &personal,
        "plans.answer",
        &["--path", asked, "--option", "revise"],
    );
    assert_eq!(code, 0, "{document}");
    planned(&personal, 2);
    let revised = ask(&personal);
    assert_eq!(revised["outcome"], "filed", "{revised}");
    assert_ne!(revised["path"], first["path"]);
    let approve = revised["approve"].as_str().unwrap();
    assert!(approve.contains("/changes/t/plans/2"), "{approve}");
}
