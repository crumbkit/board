//! A report and a failure read as Board prints them, and a printed
//! document is told apart by its schema.

use serde_json::{Value, json};

use super::*;
use crate::{File, PutReport};

/// The failure `board get /zz --json` prints in a get report's `failures`.
fn not_found() -> Value {
    json!({
        "key": "/zz",
        "index": 4,
        "code": "board::path::not_found",
        "message": "nothing is at /zz",
        "fields": {"ancestor": "/", "path": "/zz"},
        "recovery": {"kind": "hint", "text": "run `board ls /` to see what is there; paths leave out `.json`"}
    })
}

/// A put report with no write, as `board put --batch` prints for empty input.
fn empty_put() -> Value {
    json!({"schema": "board/put-report/v2", "data": {"commit": null, "writes": []}})
}

#[test]
fn a_failure_reads_every_field_and_writes_back_the_same_json() {
    let mut every = not_found();
    every["cause"] = json!("a: b");
    every["fields"]["nested"] = json!({"list": [1, -2, true, null, "x"], "big": u64::MAX});
    every["labels"] = json!([{"offset": 3, "length": 2, "line": 1, "column": 4, "name": "a.json", "text": "here", "excerpt": {"line": 1, "text": "{\"a\"}"}}]);
    every["related"] = json!([{"message": "m", "severity": "warning", "code": "c", "related": [{"message": "n", "severity": "advice"}]}]);
    every["alternatives"] = json!([{"kind": "run", "call": "board ls <ID>...", "all": "path"}, {"kind": "retry"}, {"kind": "none", "reason": "r"}]);
    every["url"] = json!("https://example.com/");
    for value in [not_found(), every] {
        let failure: Failure = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&failure).unwrap(), value);
    }
}

#[test]
fn a_report_with_a_field_this_crate_does_not_know_reads() {
    let mut report = empty_put();
    report["data"]["later"] = json!(1);
    report["total"] = json!(0);
    let read: Report<PutReport> = serde_json::from_value(report).unwrap();
    assert!(read.data.writes.is_empty());
    let known: Report<PutReport> = serde_json::from_value(empty_put()).unwrap();
    assert_eq!(read, known);
}

#[test]
fn printed_reads_the_report_of_its_own_schema_and_the_failure_document() {
    let printed: Printed<PutReport> = serde_json::from_value(empty_put()).unwrap();
    let Printed::Report(report) = &printed else {
        panic!("a put report is a report: {printed:?}")
    };
    assert_eq!(
        (report.schema.as_str(), report.dry_run),
        (PutReport::SCHEMA, false)
    );
    assert_eq!(serde_json::to_value(&printed).unwrap(), empty_put());
    let mut document = json!({"schema": "board/failure/v1"});
    document
        .as_object_mut()
        .unwrap()
        .extend(not_found().as_object().unwrap().clone());
    let printed: Printed<PutReport> = serde_json::from_value(document.clone()).unwrap();
    let Printed::Failure(failure) = &printed else {
        panic!("a failure document is a failure: {printed:?}")
    };
    assert_eq!(failure.code, "board::path::not_found");
    assert_eq!(serde_json::to_value(&printed).unwrap(), document);
}

#[test]
fn printed_refuses_another_schema_naming_it_and_admits_its_own() {
    let mut other = empty_put();
    other["schema"] = json!("board/check-report/v2");
    let refused = serde_json::from_value::<Printed<PutReport>>(other).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "the document's schema is board/check-report/v2, not board/put-report/v2 or board/failure/v1"
    );
    assert!(serde_json::from_value::<Printed<PutReport>>(empty_put()).is_ok());
}

#[test]
fn printed_reads_schema_after_the_data_as_crumbkits_order_puts_it_first() {
    let late = r#"{"data": {"commit": null, "writes": []}, "schema": "board/put-report/v2"}"#;
    let early = r#"{"schema": "board/put-report/v2", "data": {"commit": null, "writes": []}}"#;
    let read = |text| serde_json::from_str::<Printed<PutReport>>(text).unwrap();
    assert_eq!(read(late), read(early));
}

#[test]
fn printed_refuses_a_failure_document_without_recovery_and_admits_its_twin() {
    let mut document = not_found();
    document["schema"] = json!("board/failure/v1");
    assert!(serde_json::from_value::<Printed<PutReport>>(document.clone()).is_ok());
    document.as_object_mut().unwrap().remove("recovery");
    let refused = serde_json::from_value::<Printed<PutReport>>(document).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "the failure document lacks code, message or recovery"
    );
}

#[test]
fn a_get_report_reads_its_failures_and_skips_each_record_unread() {
    let report = json!({
        "schema": "board/get-report/v2",
        "data": [{
            "location": {"display": "/p", "path": "p.json", "pointers": [], "repository": "/r", "source": "worktree"},
            "blob": "350ab0b8b289baeaf217fc2914e660f59ee25faf",
            "text": "{}",
            "record": {"$type": "lr/project/v1", "deep": [[[]]]}
        }],
        "failures": [not_found()],
        "dry_run": true
    });
    let Printed::Report(read) = serde_json::from_value::<Printed<Vec<File>>>(report).unwrap()
    else {
        panic!("a get report is a report")
    };
    assert!(read.dry_run);
    assert_eq!(read.failures[0].index, Some(4));
    assert!(read.data[0].record.is_some());
}
