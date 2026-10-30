//! The commands agree about each entry: `check`, `query`, `ls` and `get`
//! give it the same source, status and code. A warm index answers `check`
//! and each view as a cold one, after any sequence of changes.

use std::collections::{BTreeMap, BTreeSet};

use board_scenario::generated::{Generated, generated, steps};
use proptest::prelude::*;

use super::*;

/// An entry by its source, as `check` spells it, and its path inside it.
type Key = (String, String);

/// What a command says about one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Said {
    logical: Value,
    kind: Value,
    status: Value,
    code: Value,
}

fn said(entry: &Value) -> Said {
    Said {
        logical: entry["logical"].clone(),
        kind: entry["kind"].clone(),
        status: entry["status"].clone(),
        code: entry["code"].clone(),
    }
}

fn key(source: &Value, path: &str) -> Key {
    let source = source
        .as_str()
        .map_or_else(|| source.to_string(), str::to_owned);
    (source, path.to_owned())
}

fn checked(run: &impl Fn(&[&str]) -> Output) -> BTreeMap<Key, Said> {
    let report = json(&run(&["check", "--all", "--json"]));
    report["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                key(&entry["source"], entry["path"].as_str().unwrap()),
                said(entry),
            )
        })
        .collect()
}

fn queried(run: &impl Fn(&[&str]) -> Output) -> BTreeMap<Key, Said> {
    let sql = "SELECT source, path, logical, kind, status, code FROM entries";
    let output = run(&["query", sql, "--limit", "10000", "--json"]);
    assert_eq!(output.code, 0, "{}", text(&output.stderr));
    json(&output)["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let entry = json!({
                "logical": row[2], "kind": row[3], "status": row[4], "code": row[5],
            });
            (key(&row[0], row[1].as_str().unwrap()), said(&entry))
        })
        .collect()
}

/// Every entry `ls` shows from the root down, through each pointer that
/// `ls` shows as `ok`, by where its listing says it is.
#[derive(Debug, Default, PartialEq)]
struct Listing {
    found: BTreeMap<Key, Said>,
    /// Each path `ls` could not list: the pointer `ls` showed as `ok` on the
    /// way to it, and the code listing it failed with.
    failed: Vec<(String, Option<Key>, Value)>,
}

fn listed(run: &impl Fn(&[&str]) -> Output) -> Listing {
    let mut listing = Listing::default();
    let mut seen = BTreeSet::new();
    let mut pending: Vec<(String, Option<Key>)> = vec![("/".to_owned(), None)];
    while let Some((path, through)) = pending.pop() {
        let output = run(&["ls", &path, "--all", "--json"]);
        let report = json(&output);
        if output.code != 0 {
            listing.failed.push((path, through, report["code"].clone()));
            continue;
        }
        let location = &report["summary"]["location"];
        let (source, at) = (&location["source"], location["path"].as_str().unwrap());
        if !seen.insert(key(source, at)) {
            continue;
        }
        for entry in report["data"].as_array().unwrap() {
            let logical = entry["logical"].as_str().map(str::to_owned);
            if entry["kind"] == "directory" {
                pending.extend(logical.map(|logical| (logical, None)));
                continue;
            }
            let name = entry["name"].as_str().unwrap();
            let physical = if at == name || at.ends_with(&format!("/{name}")) {
                at.to_owned()
            } else if at.is_empty() {
                name.to_owned()
            } else {
                format!("{at}/{name}")
            };
            let here = key(source, &physical);
            if entry["kind"] == "pointer" && entry["status"] == "ok" {
                pending.extend(logical.map(|logical| (logical, Some(here.clone()))));
            }
            listing.found.insert(here, said(entry));
        }
    }
    listing
}

/// Whether `query`, `ls` and `get` agree with what `check` says.
fn agree(run: &impl Fn(&[&str]) -> Output) -> Result<(), TestCaseError> {
    let checked = checked(run);
    prop_assert_eq!(&queried(run), &checked);
    let listing = listed(run);
    if let Some((path, through, code)) = listing.failed.first() {
        let said = through.as_ref().and_then(|key| checked.get(key));
        prop_assert!(
            false,
            "ls showed the pointer to {} as ok, but listing it fails with {}; check says {:?}",
            path,
            code,
            said
        );
    }
    for (key, said) in &listing.found {
        prop_assert_eq!(Some(said), checked.get(key), "ls of {:?}", key);
    }
    for key in checked.keys() {
        prop_assert!(
            listing.found.contains_key(key),
            "ls never reaches {:?}",
            key
        );
    }
    let records = checked.iter().filter(|(_, said)| said.kind == "record");
    for ((source, path), said) in records {
        let Some(logical) = said.logical.as_str() else {
            continue;
        };
        let output = run(&["get", logical, "--json"]);
        let answer = &super::answers(&output)[0];
        if output.code == 0 {
            prop_assert_eq!(&answer["location"]["source"], source.as_str());
            prop_assert_eq!(&answer["location"]["path"], path.as_str());
        }
        if !answer["code"].is_null() {
            prop_assert_eq!(&said.status, "degraded", "get of {}", logical);
            prop_assert_eq!(&answer["code"], &said.code, "get of {}", logical);
        }
    }
    Ok(())
}

/// What `query` prints for each view a check's entries reach: `entries`
/// and `records` in the order they give, `refs` and `links` in their keys'
/// order. Each answer starts with the exit code.
fn viewed(run: &impl Fn(&[&str]) -> Output) -> Vec<String> {
    [
        "SELECT * FROM entries",
        "SELECT * FROM records",
        "SELECT * FROM refs ORDER BY record, schema, ordinal",
        "SELECT * FROM links ORDER BY blob, ordinal",
    ]
    .into_iter()
    .map(|sql| printed(&run(&["query", sql, "--all", "--json"])))
    .collect()
}

/// What `check --all --json` prints, then each view.
fn answered(run: &impl Fn(&[&str]) -> Output) -> Vec<String> {
    let mut answers = vec![printed(&run(&["check", "--all", "--json"]))];
    answers.extend(viewed(run));
    answers
}

fn printed(output: &Output) -> String {
    format!("{}\n{}", output.code, text(&output.stdout))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn a_warm_index_answers_check_and_each_view_as_a_cold_one_after_each_change_in_a_sequence(
        first in generated(),
        steps in steps(),
        committed: bool,
    ) {
        let mut generated = Generated::new(CONFIG, &first, committed);
        let cache = tempfile::tempdir().unwrap();
        let directory = generated.scenario().workdir().to_path_buf();
        let warm = |arguments: &[&str]| board_cached(&directory, Some(cache.path()), arguments);
        let cold = |arguments: &[&str]| board_cached(&directory, None, arguments);
        prop_assert_eq!(answered(&warm), answered(&cold));
        for step in &steps {
            generated.apply(step);
            let cold = answered(&cold);
            // Queries read what the last check kept before this check
            // decides again, and what this check kept after it.
            prop_assert_eq!(&viewed(&warm), &cold[1..], "{:?}", step);
            prop_assert_eq!(&answered(&warm), &cold, "{:?}", step);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn the_commands_agree_about_every_entry_and_a_warm_ls_lists_as_a_cold_one(
        before in generated(),
        after in generated(),
        committed: bool,
    ) {
        let mut generated = Generated::new(CONFIG, &before, committed);
        let cache = tempfile::tempdir().unwrap();
        let directory = generated.scenario().workdir().to_path_buf();
        let warm = |arguments: &[&str]| board_cached(&directory, Some(cache.path()), arguments);
        let cold = |arguments: &[&str]| board_cached(&directory, None, arguments);
        agree(&warm)?;

        generated.write(&after);
        prop_assert_eq!(listed(&warm), listed(&cold));
        agree(&warm)?;
    }
}
