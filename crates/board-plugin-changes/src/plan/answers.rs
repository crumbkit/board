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
    /// Each field, the words it takes, and whether the answer given is
    /// `none`.
    fn fields(self) -> [(&'static str, &'static str, Option<bool>); 3] {
        [
            ("ux", "none or mockups", self.ux.map(|ux| ux == Ux::None)),
            (
                "data",
                "none or stored",
                self.data.map(|data| data == Data::None),
            ),
            (
                "lifecycle",
                "none or record",
                (self.lifecycle).map(|lifecycle| lifecycle == Lifecycle::None),
            ),
        ]
    }

    /// The fields `wanted` names that these answers leave out, each with
    /// the words it takes, such as `ux (none or mockups)`.
    pub(crate) fn left_out(self, wanted: Self) -> Vec<String> {
        (self.fields().into_iter().zip(wanted.fields()))
            .filter(|((_, _, given), (_, _, wanted))| wanted.is_some() && given.is_none())
            .map(|((field, words, _), _)| format!("{field} ({words})"))
            .collect()
    }

    /// Whether no answer a plan gives here differs from the one `wanted`
    /// names; an answer the plan leaves out differs from none.
    pub(crate) fn meet(self, wanted: Self) -> bool {
        (self.fields().into_iter().zip(wanted.fields()))
            .all(|((_, _, given), (_, _, wanted))| given.and(wanted).is_none() || given == wanted)
    }
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
        assert!(plan.meet(answers(json!({"data": "stored"}))));
        assert!(!plan.meet(answers(json!({"data": "none"}))));
        assert!(plan.meet(answers(json!({"ux": "none", "lifecycle": "none"}))));
        assert!(plan.meet(Answers::default()));
    }

    #[test]
    fn the_fields_left_out_are_those_wanted_and_not_given() {
        let wanted = answers(json!({"ux": "none", "data": "none", "lifecycle": "none"}));
        let plan = answers(json!({"data": "stored"}));
        assert_eq!(
            plan.left_out(wanted),
            ["ux (none or mockups)", "lifecycle (none or record)"]
        );
        // An answer that differs settles it, whatever else is left out.
        assert!(!plan.meet(wanted));
        // The twin that gives each answer leaves none out.
        assert_eq!(wanted.left_out(wanted), [""; 0]);
        assert!(wanted.meet(wanted));
    }
}
