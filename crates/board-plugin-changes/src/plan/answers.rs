//! A plan's typed answers to the three questions that decide which gate
//! checks apply: whether the change has a UI, whether it stores data, and
//! whether anything in it changes state. The prose sections hold the reason.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether the change has a UI: `none`, or `mockups` the owner agrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Ux {
    None,
    Mockups,
}

/// Whether the change stores data: `none`, or `stored`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Data {
    None,
    Stored,
}

/// Whether anything the change makes changes state: `none`, or a lifecycle
/// `record` that says how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Lifecycle {
    None,
    Record,
}

/// Each answer's field and the words it takes.
const WORDS: [(&str, &str); 3] = [
    ("ux", "none or mockups"),
    ("data", "none or stored"),
    ("lifecycle", "none or record"),
];

/// The answers a plan gives, and the answers a check's `when` names; each
/// is left out when not given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Answers {
    /// Whether the change has a UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ux: Option<Ux>,
    /// Whether the change stores data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<Data>,
    /// Whether anything the change makes changes state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lifecycle: Option<Lifecycle>,
}

impl Answers {
    /// Whether each of the three fields is given, in the order of [`WORDS`].
    fn given(self) -> [bool; 3] {
        [
            self.ux.is_some(),
            self.data.is_some(),
            self.lifecycle.is_some(),
        ]
    }

    /// The fields `wanted` names that these answers leave out, each with
    /// the words it takes, such as `ux (none or mockups)`.
    pub(crate) fn left_out(self, wanted: Self) -> Vec<String> {
        (WORDS.iter().zip(wanted.given()).zip(self.given()))
            .filter(|(((_, _), wanted), given)| *wanted && !given)
            .map(|(((field, words), _), _)| format!("{field} ({words})"))
            .collect()
    }

    /// Each answer given, as its field and its word, such as `data none`.
    pub(crate) fn said(self) -> Vec<String> {
        let given = [
            self.ux
                .map(|ux| if ux == Ux::None { "none" } else { "mockups" }),
            self.data
                .map(|data| if data == Data::None { "none" } else { "stored" }),
            (self.lifecycle).map(|lifecycle| match lifecycle {
                Lifecycle::None => "none",
                Lifecycle::Record => "record",
            }),
        ];
        (WORDS.iter().zip(given))
            .filter_map(|((field, _), word)| Some(format!("{field} {}", word?)))
            .collect()
    }

    /// These answers and those of `other`, where these give none.
    pub(crate) fn or(self, other: Self) -> Self {
        Self {
            ux: self.ux.or(other.ux),
            data: self.data.or(other.data),
            lifecycle: self.lifecycle.or(other.lifecycle),
        }
    }

    /// Whether a plan with these answers gives each answer `wanted` names:
    /// false when one differs, and otherwise why it cannot be told when the
    /// plan leaves one out.
    pub(crate) fn meet(self, wanted: Self) -> Result<bool, String> {
        if differ(wanted.ux, self.ux)
            || differ(wanted.data, self.data)
            || differ(wanted.lifecycle, self.lifecycle)
        {
            return Ok(false);
        }
        let left_out = self.left_out(wanted);
        if left_out.is_empty() {
            return Ok(true);
        }
        Err(format!(
            "the plan gives no typed answer for {}, which this check reads; write the plan again \
             with `board changes plan` and `answers` on stdin",
            left_out.join(" and ")
        ))
    }
}

/// Whether a plan's answer is given and is not the one wanted.
fn differ<Answer: PartialEq>(wanted: Option<Answer>, given: Option<Answer>) -> bool {
    matches!((wanted, given), (Some(wanted), Some(given)) if wanted != given)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Answers, Data, Lifecycle, Ux};

    fn answers(given: serde_json::Value) -> Answers {
        serde_json::from_value(given).unwrap()
    }

    #[test]
    fn each_answer_reads_its_two_words_and_refuses_another() {
        let all = answers(json!({"ux": "mockups", "data": "stored", "lifecycle": "record"}));
        assert_eq!(all.ux, Some(Ux::Mockups));
        assert_eq!(all.data, Some(Data::Stored));
        assert_eq!(all.lifecycle, Some(Lifecycle::Record));
        let none = answers(json!({"ux": "none", "data": "none", "lifecycle": "none"}));
        assert_eq!(none.ux, Some(Ux::None));
        assert_eq!(serde_json::to_value(none).unwrap()["data"], "none");
        assert!(serde_json::from_value::<Answers>(json!({"data": "None:"})).is_err());
    }

    #[test]
    fn a_plan_meets_the_answers_a_check_names_when_each_is_equal() {
        let plan = answers(json!({"ux": "none", "data": "stored", "lifecycle": "none"}));
        assert_eq!(plan.meet(answers(json!({"data": "stored"}))), Ok(true));
        assert_eq!(plan.meet(answers(json!({"data": "none"}))), Ok(false));
        assert_eq!(
            plan.meet(answers(json!({"ux": "none", "lifecycle": "none"}))),
            Ok(true)
        );
        assert_eq!(plan.meet(Answers::default()), Ok(true));
    }

    #[test]
    fn a_plan_that_leaves_out_a_named_answer_says_to_write_the_plan_again() {
        let plan = answers(json!({"ux": "none"}));
        let why = plan.meet(answers(json!({"data": "none"}))).unwrap_err();
        assert!(why.contains("data (none or stored)"), "{why}");
        assert!(why.contains("board changes plan"), "{why}");
        // The twin that gives the answer is told apart.
        let given = answers(json!({"ux": "none", "data": "none"}));
        assert_eq!(given.meet(answers(json!({"data": "none"}))), Ok(true));
        // An answer that differs settles it, whatever else is left out.
        assert_eq!(
            plan.meet(answers(json!({"ux": "mockups", "data": "none"}))),
            Ok(false)
        );
    }

    #[test]
    fn the_fields_left_out_are_those_wanted_and_not_given() {
        let wanted = answers(json!({"ux": "none", "data": "none", "lifecycle": "none"}));
        let plan = answers(json!({"data": "stored"}));
        assert_eq!(
            plan.left_out(wanted),
            ["ux (none or mockups)", "lifecycle (none or record)"]
        );
        assert_eq!(
            Answers::default().or(plan).or(wanted),
            answers(json!({"ux": "none", "data": "stored", "lifecycle": "none"}))
        );
    }
}
