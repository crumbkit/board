//! The fields of a question and a decision that a command-line `board put`
//! fills when the writer leaves them out: who wrote it, from the put's
//! session; when, from the put's time; and a question's status, `open`.

use board_ops::{Fill, With};
use board_plugin::shape::Record;

use crate::records::{DecisionRecord, QuestionRecord};

pub(crate) static FILLS: [Fill; 5] = [
    Fill {
        record_type: QuestionRecord::TYPE,
        field: "by",
        with: With::Session,
    },
    Fill {
        record_type: QuestionRecord::TYPE,
        field: "asked",
        with: With::Now,
    },
    Fill {
        record_type: QuestionRecord::TYPE,
        field: "status",
        with: With::Text("open"),
    },
    Fill {
        record_type: DecisionRecord::TYPE,
        field: "by",
        with: With::Session,
    },
    Fill {
        record_type: DecisionRecord::TYPE,
        field: "decided",
        with: With::Now,
    },
];

#[cfg(test)]
mod tests;
