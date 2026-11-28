//! The views agents query: a lookup by logical path, by record type or by
//! blob reads an index, and each view shows the same rows whether an entry's
//! decision was kept or made this run.

use board_model::{Canonical, Digest, RecordType};
use serde_json::Value;

use super::*;

/// The record `tasks/a` holds, and `tasks/copy` holds again.
const RECORD: u8 = 1;
/// The schema the record was checked against.
const SCHEMA: u8 = 2;
/// The Markdown file `notes.md` holds.
const NOTES: u8 = 3;
/// A record no entry holds, whose blob fact and body the cache keeps.
const GONE: u8 = 4;
/// A schema no entry checks the record against.
const OTHER: u8 = 5;
/// A Markdown file no entry holds, whose links the cache keeps.
const LOST: u8 = 6;

/// The commit each source is, one per entry of `kept`. Kept sources reach
/// the same kept decision, as mounts of one tree do.
fn sources(kept: &[bool]) -> impl Iterator<Item = (Source, bool)> {
    (8..)
        .zip(kept)
        .map(|(byte, kept)| (Source::Commit(blob(byte)), *kept))
}

/// The entries one source holds: a record at two paths, a Markdown file and
/// a file that could not be read, which has no blob and a code. A kept
/// decision names a tree, so both sources show the same paths.
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
        Resolved {
            blob: None,
            size: None,
            role: None,
            code: Some(Code::JsonParse),
            ..entry("broken.json", 0, Role::Json, Kind::Json)
        },
    ]
}

/// An index holding the facts of every entry, and facts the cache keeps that
/// no entry reaches, with the sources in `kept` read from a kept decision and
/// the others decided this run.
fn view(kept: &[bool]) -> View {
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
    unreached(&writer, &body, &valid);
    let key = Digest::of(b"tree");
    let mut fresh = Vec::new();
    for (source, kept) in sources(kept) {
        let entries = entries(&source);
        if kept {
            let rows = entries.iter().map(|entry| (entry, None));
            writer.put_decision(&key, rows).unwrap();
            writer.reach(&source, Some(&key), None).unwrap();
        } else {
            writer.reach(&source, None, None).unwrap();
            fresh.extend(entries);
        }
    }
    writer.decide(fresh).unwrap().view().unwrap()
}

/// Facts the cache keeps that no entry reaches: a record's blob fact and body
/// under a blob no entry holds, the record checked against a schema no entry
/// names, the record's bytes read as Markdown, a body for the Markdown file,
/// which no entry holds as a record, and the links of a Markdown file no
/// entry holds.
fn unreached(writer: &Writer, body: &Canonical, valid: &ValidationFact) {
    writer.put_blob(blob(GONE), Role::Json, &record()).unwrap();
    writer.put_body(blob(GONE), body).unwrap();
    writer
        .put_validation(blob(RECORD), blob(OTHER), valid)
        .unwrap();
    writer
        .put_blob(blob(RECORD), Role::Markdown, &markdown())
        .unwrap();
    writer.put_body(blob(NOTES), body).unwrap();
    writer
        .put_blob(blob(LOST), Role::Markdown, &markdown())
        .unwrap();
}

/// Every row `sql` returns, in a fixed order.
fn sorted(view: &View, sql: &str) -> Vec<Vec<Value>> {
    let mut rows = view.query(sql, 1000, None, LIMIT).unwrap().rows;
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

/// The lines of the plan SQLite chooses for `sql`.
fn plan(view: &View, sql: &str) -> Vec<String> {
    let explained = format!("EXPLAIN QUERY PLAN {sql}");
    let rows = view.query(&explained, 1000, None, LIMIT).unwrap().rows;
    (rows.into_iter())
        .map(|row| row[3].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_lookup_by_logical_path_or_by_record_type_reads_an_index_and_scans_no_entry_table() {
    // About as many sources as a personal board with its mounts reaches.
    // With only one or two, SQLite may scan `r` and read every kept row of
    // each source's decision, so the plan below would not be the one a real
    // board gets.
    let mut kept = vec![true; 39];
    kept.push(false);
    let view = view(&kept);
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
        // The kept arm is the lines from the left-most subquery to the
        // UNION ALL. Its first search must use the index the filter names;
        // a search by decision key first reads every kept row.
        let first = (lines.iter())
            .skip_while(|line| *line != "LEFT-MOST SUBQUERY")
            .take_while(|line| *line != "UNION ALL")
            .find(|line| line.starts_with("SEARCH "));
        assert!(
            first.is_some_and(|line| line.contains(index)),
            "{sql} does not start the kept rows from {index}: {lines:#?}"
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
    let views = [[true, true], [true, false], [false, false]].map(|kept| view(&kept));
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
    let [gone, other, lost, record, notes] =
        [GONE, OTHER, LOST, RECORD, NOTES].map(|byte| blob(byte).to_string());
    for (table, filter) in [
        (
            "blobs",
            format!("blob IN ('{gone}', '{lost}') OR (blob = '{record}' AND role = 'markdown')"),
        ),
        ("bodies", format!("blob IN ('{gone}', '{notes}')")),
        ("validations", format!("schema = '{other}'")),
        ("refs", format!("schema = '{other}'")),
        ("links", format!("blob IN ('{lost}', '{record}')")),
    ] {
        let sql = format!("SELECT * FROM {table} WHERE {filter}");
        for view in &views {
            let rows = sorted(view, &sql);
            assert!(
                rows.is_empty(),
                "{table} shows what nothing reaches: {rows:?}"
            );
        }
    }
    let sql = "SELECT status, code FROM entries WHERE path = 'broken.json'";
    let degraded = vec![
        Value::from("degraded"),
        Value::from(Code::JsonParse.as_str()),
    ];
    for view in &views {
        assert_eq!(sorted(view, sql), vec![degraded.clone(); 2], "{sql}");
    }
}
