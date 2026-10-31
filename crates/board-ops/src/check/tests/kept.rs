//! What the index keeps of a checked entry beyond its row: written and read
//! back unchanged, and refused with the index code when it cannot be read.

use board_index::KeptRow;
use board_model::{Change, Code, Kind, LogicalPath, ObjectId, Source};
use board_tree::Status;
use proptest::prelude::*;

use crate::check::{from_kept, to_kept};
use crate::{Checked, Detail};

fn logical() -> impl Strategy<Value = LogicalPath> {
    prop::collection::vec("[a-z.]{1,6}", 1..4).prop_filter_map("a logical path", |segments| {
        format!("/{}", segments.join("/")).parse().ok()
    })
}

fn detail() -> impl Strategy<Value = Detail> {
    let text = || prop::option::of(any::<String>());
    (
        (text(), any::<String>(), any::<u64>()),
        (
            prop::option::of(logical()),
            prop::option::of(logical()),
            text(),
        ),
    )
        .prop_map(
            |((pointer, reason, failures), (at, mount, target))| Detail {
                pointer,
                reason,
                failures,
                at,
                mount,
                target,
            },
        )
}

fn checked() -> impl Strategy<Value = Checked> {
    (
        prop::option::of(logical()),
        prop::sample::select(Kind::ALL.to_vec()),
        prop::option::of(prop::sample::select(Code::ALL.to_vec())),
        prop::option::of(detail()),
        prop::option::of(any::<String>()),
    )
        .prop_map(|(logical, kind, code, detail, schema)| Checked {
            source: Source::Commit(ObjectId::Sha1([7; 20])),
            path: "tasks/a.json".to_owned(),
            logical,
            kind,
            status: code.map_or(Status::Ok, Status::Degraded),
            detail,
            change: Change::Committed,
            schema,
        })
}

/// `checked` as a kept row holds it.
fn row(checked: &Checked, detail: Option<String>) -> KeptRow {
    KeptRow {
        path: checked.path.clone(),
        logical: checked.logical.clone(),
        kind: checked.kind,
        code: match checked.status {
            Status::Degraded(code) => Some(code),
            Status::Ok => None,
        },
        detail,
    }
}

proptest! {
    #[test]
    fn a_checked_entry_kept_and_read_back_is_the_same_entry(checked in checked()) {
        let kept = row(&checked, to_kept(&checked));
        prop_assert_eq!(from_kept(&checked.source, kept).unwrap(), checked);
    }
}

#[test]
fn a_kept_detail_that_does_not_read_fails_with_the_index_code_and_its_twin_reads() {
    let checked = Checked {
        source: Source::Commit(ObjectId::Sha1([7; 20])),
        path: "tasks/a.json".to_owned(),
        logical: Some("/vendor/tasks/a".parse().unwrap()),
        kind: Kind::Record,
        status: Status::Degraded(Code::SchemaInvalid),
        detail: None,
        change: Change::Committed,
        schema: Some(".board/types/lr/gone/v1.schema.json".to_owned()),
    };
    let schema = r#""schema": ".board/types/lr/gone/v1.schema.json""#;
    let detail = r#""reason": "is gone", "failures": 1"#;
    let unknown = format!(r#"{{"detail": {{{detail}, "since": 2}}, {schema}}}"#);
    for (kept, read) in [
        (format!(r#"{{"detail": null, {schema}}}"#), true),
        (r#"{"detail": null, "schema": 7}"#.to_owned(), false),
        (format!(r#"{{"detail": {{{detail}}}, {schema}}}"#), true),
        (unknown, false),
    ] {
        let read_back = from_kept(&checked.source, row(&checked, Some(kept.clone())));
        if read {
            assert!(read_back.is_ok(), "{kept}");
        } else {
            let error = read_back.unwrap_err();
            assert_eq!(error.code(), Code::IndexFailed, "{kept}");
            assert_eq!(
                error.to_string(),
                format!(
                    "the index keeps detail {kept:?} for /vendor/tasks/a \
                     (tasks/a.json in its source), which cannot be read"
                ),
            );
        }
    }
}
