//! A run and a suite read and write as Board stores them.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;

/// A run with only its required fields.
fn bare() -> Run {
    Run {
        tag: Tag::new(),
        suite: "/suites/a".to_owned(),
        by: None,
        started: "2026-10-01T09:00:00Z".to_owned(),
        scope: Scope::Full,
        results: vec![CaseResult {
            case: "a".to_owned(),
            outcome: Some(Outcome::Pass),
            measures: None,
            note: None,
            trial: None,
            evidence: None,
        }],
        dirty: None,
        finished: None,
        versions: None,
        host: None,
        measures: None,
        setup: None,
        summary: None,
        written_by: None,
    }
}

#[test]
fn a_run_with_every_optional_field_left_out_writes_no_null() {
    assert_eq!(
        serde_json::to_value(bare()).unwrap(),
        json!({
            "$type": "runs/run/v1",
            "suite": "/suites/a",
            "started": "2026-10-01T09:00:00Z",
            "scope": "full",
            "results": [{"case": "a", "outcome": "pass"}]
        })
    );
}

#[test]
fn a_record_of_another_type_is_refused_naming_it_and_its_twin_reads() {
    let mut run = serde_json::to_value(bare()).unwrap();
    assert_eq!(serde_json::from_value::<Run>(run.clone()).unwrap(), bare());
    run["$type"] = json!("runs/suite/v1");
    let refused = serde_json::from_value::<Run>(run).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "the $type is runs/suite/v1, not runs/run/v1"
    );
}

#[test]
fn a_scope_or_outcome_a_later_board_adds_reads_as_written() {
    let mut run = serde_json::to_value(bare()).unwrap();
    run["scope"] = json!("partial");
    run["results"][0]["outcome"] = json!("flaky");
    let read: Run = serde_json::from_value(run.clone()).unwrap();
    assert_eq!(read.scope, Scope::Other("partial".to_owned()));
    assert_eq!(
        read.results[0].outcome,
        Some(Outcome::Other("flaky".to_owned()))
    );
    assert_eq!(serde_json::to_value(read).unwrap(), run);
}

#[test]
fn a_whole_number_reads_as_an_integer_or_without_a_fraction_and_a_fraction_is_refused() {
    let host = |cpus: Value| json!({"os": "macos", "arch": "aarch64", "logical_cpus": cpus});
    for admitted in [json!(8), json!(8.0), json!(0)] {
        let read: Host = serde_json::from_value(host(admitted.clone())).unwrap();
        assert_eq!(
            read.logical_cpus,
            admitted
                .as_f64()
                .unwrap()
                .to_string()
                .parse::<u32>()
                .unwrap()
        );
    }
    for refused in [json!(8.5), json!(-1), json!(5_000_000_000_u64), json!("8")] {
        assert!(
            serde_json::from_value::<Host>(host(refused.clone())).is_err(),
            "{refused}"
        );
    }
    let result = |trial: Value| json!({"case": "a", "outcome": "pass", "trial": trial});
    let trial = |value| serde_json::from_value::<CaseResult>(result(value)).map(|read| read.trial);
    assert_eq!(trial(json!(2.0)).unwrap(), Some(2));
    assert_eq!(trial(json!(null)).unwrap(), None);
    assert!(trial(json!(1.5)).is_err());
}

#[test]
fn a_measure_is_a_whole_number_a_number_a_label_or_a_flag_and_null_is_refused() {
    let read: BTreeMap<String, MeasureValue> =
        serde_json::from_value(json!({"a": 3, "b": 2.5, "c": "fast", "d": true})).unwrap();
    let numbers: Vec<Option<f64>> = read.values().map(MeasureValue::as_f64).collect();
    assert_eq!(numbers, [Some(3.0), Some(2.5), None, None]);
    assert_eq!(serde_json::to_value(&read).unwrap()["a"], json!(3));
    assert!(serde_json::from_value::<MeasureValue>(json!(null)).is_err());
    assert!(serde_json::from_value::<MeasureValue>(json!(0)).is_ok());
}

#[test]
fn a_suite_reads_and_writes_back_the_same_json_and_skips_its_cases() {
    let suite = json!({
        "$type": "runs/suite/v1",
        "subjects": ["crumbkit/board"],
        "method": "Run it.",
        "command": "bin/test",
        "cases_file": "/suites/a/cases.jsonl",
        "measures": [{"name": "median_ms", "unit": "ms", "description": "The median."}]
    });
    let read: Suite = serde_json::from_value(suite.clone()).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), suite);
    let mut inline = suite;
    inline.as_object_mut().unwrap().remove("cases_file");
    inline["cases"] = json!([{"id": "a", "input": {"x": 1}}]);
    let read: Suite = serde_json::from_value(inline).unwrap();
    assert_eq!(read.cases_file, None);
}

fn text() -> impl Strategy<Value = String> {
    "[a-z/:0-9-]{0,12}"
}

fn measures() -> impl Strategy<Value = Option<BTreeMap<String, MeasureValue>>> {
    let value = prop_oneof![
        any::<i64>().prop_map(MeasureValue::Integer),
        (-1e12..1e12_f64).prop_map(MeasureValue::Number),
        text().prop_map(MeasureValue::Label),
        any::<bool>().prop_map(MeasureValue::Flag),
    ];
    proptest::option::of(proptest::collection::btree_map(text(), value, 0..3))
}

fn outcome() -> impl Strategy<Value = Option<Outcome>> {
    proptest::option::of(prop_oneof![
        Just(Outcome::Pass),
        Just(Outcome::Fail),
        Just(Outcome::Error),
        Just(Outcome::Skipped),
        "x[a-z]{0,6}".prop_map(Outcome::Other),
    ])
}

fn result() -> impl Strategy<Value = CaseResult> {
    let notes = proptest::option::of(text());
    let evidence = proptest::option::of(proptest::collection::vec(text(), 0..2));
    let trial = proptest::option::of(any::<u32>());
    (text(), outcome(), measures(), notes, trial, evidence).prop_map(
        |(case, outcome, measures, note, trial, evidence)| CaseResult {
            case,
            outcome,
            measures,
            note,
            trial,
            evidence,
        },
    )
}

fn actor() -> impl Strategy<Value = Option<Actor>> {
    let part = || proptest::option::of(text());
    proptest::option::of((part(), part(), part(), part()).prop_map(
        |(session, program, person, agent)| Actor {
            session,
            program,
            person,
            agent,
        },
    ))
}

fn host() -> impl Strategy<Value = Option<Host>> {
    proptest::option::of(
        (text(), text(), any::<u32>()).prop_map(|(os, arch, logical_cpus)| Host {
            os,
            arch,
            logical_cpus,
        }),
    )
}

fn run() -> impl Strategy<Value = Run> {
    let scope = prop_oneof![
        Just(Scope::Full),
        Just(Scope::Targeted),
        "x[a-z]{0,6}".prop_map(Scope::Other),
    ];
    let optional = || proptest::option::of(text());
    let versions = proptest::option::of(proptest::collection::btree_map(text(), text(), 0..3));
    let results = proptest::collection::vec(result(), 0..3);
    let dirty = proptest::option::of(any::<bool>());
    (
        (text(), actor(), text(), scope, results, dirty),
        (optional(), versions, host(), measures()),
        (optional(), optional(), optional()),
    )
        .prop_map(
            |(
                (suite, by, started, scope, results, dirty),
                (finished, versions, host, measures),
                (setup, summary, written_by),
            )| Run {
                tag: Tag::new(),
                suite,
                by,
                started,
                scope,
                results,
                dirty,
                finished,
                versions,
                host,
                measures,
                setup,
                summary,
                written_by,
            },
        )
}

proptest! {
    #[test]
    fn any_run_reads_back_as_the_run_it_wrote(run in run()) {
        let text = serde_json::to_string(&run).unwrap();
        prop_assert_eq!(serde_json::from_str::<Run>(&text).unwrap(), run);
    }
}
