//! Each derived state is reached from records written in order, and each
//! has a twin that does not reach it.

use board_ops::{Limit, QueryError, QueryReport, Settings};
use serde_json::json;

use super::*;
use crate::standing::DECISIONS;

/// A board's decisions and questions, which a host answers queries from.
#[derive(Default)]
struct Written {
    decisions: Vec<Vec<Value>>,
    questions: Vec<Vec<Value>>,
}

struct Answering(Written, Settings);

impl Host for Answering {
    fn query(&self, sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        let rows = match sql {
            DECISIONS => self.0.decisions.clone(),
            QUESTIONS => self.0.questions.clone(),
            other => panic!("unexpected query {other}"),
        };
        Ok(QueryReport {
            columns: vec!["logical".into(), "code".into(), "body".into()],
            rows,
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("this plugin resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("this plugin reads no files")
    }

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("this plugin writes no records")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("this plugin calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("this plugin opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("this plugin asks the owner nothing")
    }

    fn settings(&self) -> &Settings {
        &self.1
    }
}

impl Written {
    fn decision(mut self, path: &str, decided: &str, fields: &Value) -> Self {
        let mut record = json!({
            "$type": "decisions/decision/v1",
            "decision": format!("Decide {path}."),
            "why": "Because.",
            "decided": decided,
            "session": "local_0a1b",
        });
        for (name, value) in fields.as_object().unwrap() {
            record[name] = value.clone();
        }
        (self.decisions).push(vec![json!(path), Value::Null, json!(record.to_string())]);
        self
    }

    fn question(mut self, path: &str, status: &str, belongs_to: &[&str]) -> Self {
        let record = json!({
            "$type": "plans/question/v1",
            "question": "Review these?",
            "session": "local_0a1b",
            "asked": "2026-10-21T09:00:00Z",
            "status": status,
            "for": belongs_to,
        });
        (self.questions).push(vec![json!(path), Value::Null, json!(record.to_string())]);
        self
    }

    fn call(
        self,
        capability: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
        input: &Value,
    ) -> Value {
        let settings = Settings {
            now: std::time::SystemTime::UNIX_EPOCH,
            day_start: None,
            caller: None,
            environment: board_ops::Variables::default(),
        };
        capability(&Answering(self, settings), input).unwrap()
    }
}

const A: &str = "/projects/p/decisions/a";
const B: &str = "/projects/p/decisions/b";

/// Each listed decision's path and state, as (path, review, superseded).
fn states(output: &Value) -> Vec<(String, String, bool)> {
    (output["decisions"].as_array().unwrap().iter())
        .map(|listed| {
            (
                listed["path"].as_str().unwrap().to_owned(),
                listed["review"].as_str().unwrap().to_owned(),
                listed["superseded"].as_bool().unwrap(),
            )
        })
        .collect()
}

/// `about` on an eval the decision rests on, which lists every state but
/// superseded.
fn about_eval(written: Written) -> Vec<(String, String, bool)> {
    states(&written.call(about, &json!({"ref": "/evals/e"})))
}

fn on_eval() -> Written {
    Written::default().decision(A, "2026-10-20T09:00:00Z", &json!({"basis": ["/evals/e"]}))
}

#[test]
fn a_decision_no_question_names_is_unreviewed_and_not_open() {
    let written = on_eval();
    assert_eq!(
        about_eval(written),
        [(A.into(), "unreviewed".into(), false)]
    );
    assert_eq!(on_eval().call(open, &json!({}))["total"], 0);
}

#[test]
fn an_open_question_whose_for_names_a_decision_makes_it_await_review_and_one_for_another_does_not()
{
    let asked = on_eval().question("/projects/p/questions/q", "open", &[A]);
    let output = asked.call(open, &json!({}));
    assert_eq!(states(&output), [(A.into(), "awaiting".into(), false)]);
    assert_eq!(
        output["decisions"][0]["questions"],
        json!(["/projects/p/questions/q"])
    );
    let other = on_eval().question("/projects/p/questions/q", "open", &[B]);
    assert_eq!(about_eval(other), [(A.into(), "unreviewed".into(), false)]);
}

#[test]
fn an_answered_question_makes_a_decision_reviewed_and_a_withdrawn_one_leaves_it_unreviewed() {
    let answered = on_eval().question("/projects/p/questions/q", "answered", &[A]);
    assert_eq!(about_eval(answered), [(A.into(), "reviewed".into(), false)]);
    let withdrawn = on_eval().question("/projects/p/questions/q", "withdrawn", &[A]);
    assert_eq!(
        about_eval(withdrawn),
        [(A.into(), "unreviewed".into(), false)]
    );
}

#[test]
fn a_new_open_question_after_an_answered_one_makes_the_decision_await_review_again() {
    let again = (on_eval().question("/projects/p/questions/q1", "answered", &[A])).question(
        "/projects/p/questions/q2",
        "open",
        &[A],
    );
    assert_eq!(about_eval(again), [(A.into(), "awaiting".into(), false)]);
}

#[test]
fn a_later_decision_that_builds_on_one_does_not_review_it() {
    let built_on = on_eval().decision(B, "2026-10-21T09:00:00Z", &json!({"basis": [A]}));
    let output = built_on.call(about, &json!({"ref": A}));
    assert_eq!(states(&output), [(B.into(), "unreviewed".into(), false)]);
    assert_eq!(
        about_eval(on_eval()),
        [(A.into(), "unreviewed".into(), false)]
    );
}

#[test]
fn a_decision_that_supersedes_another_leaves_it_out_of_about_and_one_that_names_it_otherwise_does_not()
 {
    let superseding = on_eval().decision(
        B,
        "2026-10-21T09:00:00Z",
        &json!({"supersedes": [A], "basis": ["/evals/e"]}),
    );
    assert_eq!(
        about_eval(superseding),
        [(B.into(), "unreviewed".into(), false)]
    );
    let building = on_eval().decision(
        B,
        "2026-10-21T09:00:00Z",
        &json!({"basis": ["/evals/e", A]}),
    );
    assert_eq!(
        about_eval(building),
        [
            (B.into(), "unreviewed".into(), false),
            (A.into(), "unreviewed".into(), false)
        ]
    );
}

#[test]
fn open_lists_a_superseded_decision_still_awaiting_review_and_says_it_is_superseded() {
    let written = (on_eval().decision(B, "2026-10-21T09:00:00Z", &json!({"supersedes": [A]})))
        .question("/projects/p/questions/q", "open", &[A, B]);
    assert_eq!(
        states(&written.call(open, &json!({}))),
        [
            (A.into(), "awaiting".into(), true),
            (B.into(), "awaiting".into(), false)
        ]
    );
}

#[test]
fn open_lists_oldest_first_under_its_path_and_about_newest_first_under_a_directory() {
    let written = || {
        Written::default()
            .decision(
                B,
                "2026-10-19T09:00:00+02:00",
                &json!({"basis": ["/repos/r/docs/x.md"]}),
            )
            .decision(
                A,
                "2026-10-19T08:00:00Z",
                &json!({"basis": ["/repos/r/docs"]}),
            )
            .decision(
                "/projects/q/decisions/c",
                "2026-10-18T08:00:00Z",
                &json!({"basis": ["/repos/rr"]}),
            )
            .question(
                "/projects/p/questions/q",
                "open",
                &[A, B, "/projects/q/decisions/c"],
            )
    };
    let oldest = written().call(open, &json!({"path": "/projects/p"}));
    let paths: Vec<_> = states(&oldest).into_iter().map(|(path, ..)| path).collect();
    assert_eq!(paths, [B, A], "09:00+02:00 is 07:00Z, before 08:00Z");
    let newest = written().call(about, &json!({"ref": "/repos/r/docs/"}));
    let paths: Vec<_> = states(&newest).into_iter().map(|(path, ..)| path).collect();
    assert_eq!(paths, [A, B]);
    let repository = written().call(about, &json!({"ref": "/repos/r"}));
    assert_eq!(repository["total"], 2, "/repos/rr is not under /repos/r");
}

#[test]
fn the_limit_cuts_the_list_and_says_so() {
    let written = on_eval()
        .decision(B, "2026-10-21T09:00:00Z", &json!({"basis": ["/evals/e"]}))
        .question("/projects/p/questions/q", "open", &[A, B]);
    let output = written.call(open, &json!({"limit": 1}));
    assert_eq!(
        (&output["total"], &output["truncated"]),
        (&json!(2), &json!(true))
    );
    assert_eq!(states(&output).len(), 1);
}

#[test]
fn a_decision_board_marks_degraded_or_without_a_time_is_left_out_and_a_dangling_one_is_listed() {
    let mut written = on_eval().decision(B, "yesterday", &json!({"basis": ["/evals/e"]}));
    written.decisions.push(vec![
        json!("/projects/p/decisions/c"),
        json!("board::record::invalid"),
        json!(written.decisions[0][2].clone()),
    ]);
    written.decisions.push(vec![
        json!("/projects/p/decisions/d"),
        json!("board::ref::dangling"),
        json!(written.decisions[0][2].clone()),
    ]);
    let output = written.call(about, &json!({"ref": "/evals/e"}));
    let paths: Vec<_> = states(&output).into_iter().map(|(path, ..)| path).collect();
    assert_eq!(paths, [A, "/projects/p/decisions/d"]);
    assert_eq!(output["unreadable"], 2, "B has no time and c is invalid");
    assert_eq!(
        on_eval().call(about, &json!({"ref": "/evals/e"}))["unreadable"],
        0
    );
}

#[test]
fn a_question_board_marks_degraded_or_without_a_status_is_counted_unreadable_and_a_dangling_one_is_read()
 {
    let mut written = on_eval().question("/projects/p/questions/q", "open", &[A]);
    let body = written.questions[0][2].clone();
    written.questions.push(vec![
        json!("/projects/p/questions/invalid"),
        json!("board::record::invalid"),
        body.clone(),
    ]);
    written.questions.push(vec![
        json!("/projects/p/questions/dangling"),
        json!("board::ref::dangling"),
        body,
    ]);
    written = written.question("/projects/p/questions/odd", "someday", &[A]);
    let output = written.call(open, &json!({}));
    assert_eq!(
        output["unreadable"], 2,
        "the invalid question and the one whose status is unknown"
    );
    assert_eq!(
        output["decisions"][0]["questions"],
        json!(["/projects/p/questions/dangling", "/projects/p/questions/q"])
    );
    let clean = on_eval()
        .question("/projects/p/questions/q", "open", &[A])
        .call(open, &json!({}));
    assert_eq!(clean["unreadable"], 0);
}
