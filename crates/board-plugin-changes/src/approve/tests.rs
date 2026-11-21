//! `changes.approve` asks only once an attached answer chose an option that
//! names the change, and reports what the owner's decide answered.

use board_model::{Code, Fields, Refusal};
use board_ops::DecideReport;
use serde_json::{Map, Value, json};

use super::{Output, State, chosen, outcome};
use crate::change::Referenced;
use crate::gates::Observed;
use crate::records::Kind;

const PATH: &str = "/changes/t";
const PLAN: &str = "/changes/t/plans/1";

/// The fixture change with an attached question whose option `a` names
/// `named`, and an attached answer that chose `a`.
fn observed(named: &str) -> Observed {
    let change = json!({
        "$type": "changes/change/v1", "title": "T", "mount": "/", "phase": "plan", "plan": PLAN,
    });
    let options = json!([{"id": "a", "text": format!("Approve the plan at {named}.")}]);
    let body = |value: Value| -> Map<String, Value> { value.as_object().unwrap().clone() };
    let records = Referenced {
        records: [
            (
                "/q/1".to_owned(),
                (
                    "plans/question/v1".to_owned(),
                    body(json!({"options": options})),
                ),
            ),
            (
                "/a/1".to_owned(),
                (
                    "plans/answer/v1".to_owned(),
                    body(json!({"question": "/q/1", "choice": "a"})),
                ),
            ),
        ]
        .into_iter()
        .collect(),
        unreadable: Vec::new(),
        unreadable_types: std::collections::BTreeMap::new(),
    };
    let change = crate::change::from_value(change).unwrap();
    Observed::new(PATH, change, Kind::Build, records)
}

#[test]
fn approve_refuses_without_an_answer_whose_option_names_the_change_and_asks_with_one() {
    let refused = chosen(&observed("/changes/other")).unwrap_err();
    assert_eq!(refused.code, "changes::approve::not_chosen");
    assert_eq!(chosen(&observed(PATH)).unwrap(), PLAN);
    let mut unplanned = observed(PATH);
    unplanned.change.plan = None;
    assert_eq!(
        chosen(&unplanned).unwrap_err().code,
        "changes::approve::no_plan"
    );
}

fn answered(code: Code) -> Result<DecideReport, Box<Refusal>> {
    let fields = Fields {
        effect: Some("e1".to_owned()),
        ..Fields::default()
    };
    Err(Box::new(Refusal::new(code, "decided", &fields)))
}

fn output(effect: &str, state: State) -> Output {
    Output {
        path: PATH.to_owned(),
        plan: PLAN.to_owned(),
        effect: Some(effect.to_owned()),
        state,
    }
}

#[test]
fn the_decide_answers_asked_refused_or_approved_and_any_other_refusal_is_passed_on() {
    let of = |decided| outcome(PATH.to_owned(), PLAN.to_owned(), decided);
    assert_eq!(
        of(answered(Code::SettingsAsked)).unwrap(),
        output("e1", State::Asked)
    );
    assert_eq!(
        of(answered(Code::SettingsRefused)).unwrap(),
        output("e1", State::Refused)
    );
    let approved = DecideReport {
        capability: "changes.approve".to_owned(),
        path: PATH.parse().unwrap(),
        effect: Some("e1".to_owned()),
    };
    assert_eq!(of(Ok(approved)).unwrap(), output("e1", State::Approved));
    let other = of(answered(Code::SettingsNoPersonal)).unwrap_err();
    assert_eq!(other.code, "changes::board::refused");
}
