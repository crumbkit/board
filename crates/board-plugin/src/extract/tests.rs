use super::*;

fn record(n: usize, text: &str) -> Record {
    Record {
        blob: ObjectId::from_hex(&format!("{n:040x}")).unwrap(),
        path: format!("/tasks/{n}"),
        record_type: "lr/task/v1".to_owned(),
        record: json!({"$type": "lr/task/v1", "text": text}),
    }
}

fn sizes(batches: &[Vec<(Record, Value)>]) -> Vec<usize> {
    batches.iter().map(Vec::len).collect()
}

#[test]
fn a_batch_holds_at_most_a_thousand_records_in_order() {
    let records: Vec<Record> = (0..2_500).map(|n| record(n, "")).collect();
    let batches = batches(records, Limits::default());
    assert_eq!(sizes(&batches), [1_000, 1_000, 500]);
    assert_eq!(batches[1][0].0.path, "/tasks/1000");
    assert_eq!(batches[2][0].1["blob"], format!("{:040x}", 2_000));
}

#[test]
fn a_batch_holds_at_most_sixteen_mebibytes_of_records() {
    let large = "x".repeat(6 * 1024 * 1024);
    let records: Vec<Record> = (0..5).map(|n| record(n, &large)).collect();
    assert_eq!(sizes(&batches(records, Limits::default())), [2, 2, 1]);
    let small = "x".repeat(5 * 1024 * 1024);
    let records: Vec<Record> = (0..5).map(|n| record(n, &small)).collect();
    assert_eq!(sizes(&batches(records, Limits::default())), [3, 2]);
}

#[test]
fn the_caller_sets_the_batch_limits() {
    let records: Vec<Record> = (0..5).map(|n| record(n, "")).collect();
    let limits = Limits {
        batch_records: 2,
        ..Limits::default()
    };
    assert_eq!(sizes(&batches(records, limits)), [2, 2, 1]);
}

#[test]
fn no_records_make_no_batches() {
    assert!(batches(Vec::new(), Limits::default()).is_empty());
}
