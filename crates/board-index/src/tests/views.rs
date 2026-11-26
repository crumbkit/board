//! The views agents query: a lookup by logical path, by record type or by
//! blob reads an index, and each view shows the same rows whether an entry's
//! decision was kept or made this run.

use board_model::{Digest, RecordType};
use serde_json::Value;

use super::*;

/// The record `tasks/a` holds, and `tasks/copy` holds again.
const RECORD: u8 = 1;
/// The schema the record was checked against.
const SCHEMA: u8 = 2;
/// The Markdown file `notes.md` holds.
const NOTES: u8 = 3;

/// The two commits that reach the same kept decision, as two mounts of one
/// tree do.
fn sources() -> [Source; 2] {
    [Source::Commit(blob(8)), Source::Commit(blob(9))]
}

/// The entries one source holds: a record at two paths and a Markdown file.
/// A kept decision names a tree, so both sources show the same paths.
fn entries(source: &Source) -> Vec<Resolved> {
    let entry = |path: &str, object: u8, role, kind| Resolved {
        source: source.clone(),
        path: path.to_owned(),
        logical: Some(format!("/vendor/{path}").parse().unwrap()),
        blob: Some(blob(object)),
        size: Some(2),
        role: Some(role),
        schema: (kind == Kind::Record).then(|| blob(SCHEMA)),
        kind,
        code: None,
        change: board_model::Change::Committed,
    };
    vec![
        entry("tasks/a", RECORD, Role::Json, Kind::Record),
        entry("tasks/copy", RECORD, Role::Json, Kind::Record),
        entry("notes.md", NOTES, Role::Markdown, Kind::File),
    ]
}

/// An index holding the facts of every entry, with the sources in `kept`
/// read from a kept decision and the others decided this run.
fn view(kept: [bool; 2]) -> View {
    let writer = Index::fall_back().unwrap().write().unwrap();
    writer
        .put_blob(blob(RECORD), Role::Json, &record())
        .unwrap();
    let body = canonical_json(&serde_json::json!({"$type": "test/task/v1"}));
    writer.put_body(blob(RECORD), &body).unwrap();
    let reference = RefFact {
        pointer: "/blocks".to_owned(),
        target: "/tasks/b".parse().unwrap(),
        accepts: Some(vec!["test/task/v1".parse::<RecordType>().unwrap()]),
        local: true,
    };
    let valid = ValidationFact::Valid(vec![reference]);
    writer
        .put_validation(blob(RECORD), blob(SCHEMA), &valid)
        .unwrap();
    writer
        .put_blob(blob(NOTES), Role::Markdown, &markdown())
        .unwrap();
    let key = Digest::of(b"tree");
    let mut fresh = Vec::new();
    for (source, kept) in sources().iter().zip(kept) {
        let entries = entries(source);
        if kept {
            let rows = entries.iter().map(|entry| (entry, None));
            writer.put_decision(&key, rows).unwrap();
            writer.reach(source, Some(&key), None).unwrap();
        } else {
            writer.reach(source, None, None).unwrap();
            fresh.extend(entries);
        }
    }
    writer.decide(fresh).unwrap().view().unwrap()
}

/// Every row `sql` returns, in a fixed order.
fn sorted(view: &View, sql: &str) -> Vec<Vec<Value>> {
    let mut rows = view.query(sql, 1000, None).unwrap().rows;
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

/// The lines of the plan SQLite chooses for `sql`.
fn plan(view: &View, sql: &str) -> Vec<String> {
    let explained = format!("EXPLAIN QUERY PLAN {sql}");
    let rows = view.query(&explained, 1000, None).unwrap().rows;
    (rows.into_iter())
        .map(|row| row[3].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_lookup_by_logical_path_or_by_record_type_reads_an_index_and_scans_no_entry_table() {
    let view = view([true, false]);
    for (sql, index) in [
        (
            "SELECT e.body FROM records AS e WHERE e.logical = '/vendor/tasks/a'",
            "decided_logical",
        ),
        (
            "SELECT count(*) FROM records AS e WHERE e.type = 'test/task/v1'",
            "blobs_type",
        ),
        (
            "SELECT e.type, b.body FROM entries AS e LEFT JOIN bodies AS b ON b.blob = e.blob \
             WHERE e.logical = '/vendor/tasks/a'",
            "decided_logical",
        ),
    ] {
        let lines = plan(&view, sql);
        assert!(
            lines.iter().any(|line| line.contains(index)),
            "{sql} reads no {index}: {lines:#?}"
        );
        // `e` is the view's own rows and `r` the few sources the run
        // reached; a scan of anything else reads every entry or fact.
        let scans: Vec<&String> = (lines.iter())
            .filter(|line| {
                let scanned = line
                    .strip_prefix("SCAN ")
                    .and_then(|rest| rest.split(' ').next());
                !matches!(scanned, None | Some("e" | "r")) || line.contains("AUTOMATIC")
            })
            .collect();
        assert!(scans.is_empty(), "{sql} scans {scans:#?}");
    }
}

#[test]
fn each_view_shows_the_same_rows_whether_its_entries_were_kept_or_decided_this_run() {
    let views = [[true, true], [true, false], [false, false]].map(view);
    for table in [
        "entries",
        "records",
        "bodies",
        "blobs",
        "validations",
        "refs",
        "links",
    ] {
        let sql = format!("SELECT * FROM {table}");
        let [kept, mixed, fresh] = views.each_ref().map(|view| sorted(view, &sql));
        assert!(!kept.is_empty(), "{table} is empty");
        assert_eq!(kept, fresh, "{table}");
        assert_eq!(mixed, fresh, "{table}");
    }
}
