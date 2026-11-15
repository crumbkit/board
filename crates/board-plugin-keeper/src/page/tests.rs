//! The page's bytes: the same for the same records whatever order they
//! arrive in, equal to the golden file, and free of anything that fetches.

use std::path::Path;

use serde_json::{Value, json};

use crate::model::{Facts, Pass, build};

use super::*;

fn fixture(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("page/fixture")
        .join(name);
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

/// [`crate::model::tests`]'s reversal: every list and every object's key
/// order, with a pair inside a list kept as it is.
fn reversed(value: &Value, in_list: bool) -> Value {
    match value {
        Value::Array(items) => {
            let mut items: Vec<Value> = items.iter().map(|item| reversed(item, true)).collect();
            if !in_list {
                items.reverse();
            }
            Value::Array(items)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .rev()
                .map(|(key, value)| (key.clone(), reversed(value, false)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn rendered(pass: &Value, facts: &Value) -> String {
    let pass: Pass = serde_json::from_value(pass.clone()).unwrap();
    let facts: Facts = serde_json::from_value(facts.clone()).unwrap();
    html(&build(&pass, &facts).unwrap())
}

#[test]
fn the_page_is_byte_for_byte_the_same_when_every_input_list_is_reversed() {
    let forward = rendered(&fixture("pass.json"), &fixture("facts.json"));
    let backward = rendered(
        &reversed(&fixture("pass.json"), false),
        &reversed(&fixture("facts.json"), false),
    );
    assert!(forward == backward, "the two pages differ");
}

/// The committed page holds, byte for byte, the page the fixture renders.
/// `SNAPSHOTS=overwrite` rewrites it. The Node check reads it too.
#[expect(
    clippy::disallowed_methods,
    reason = "the page is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_fixtures_page_is_what_the_committed_golden_holds() {
    let page = rendered(&fixture("pass.json"), &fixture("facts.json"));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("page/fixture/page.html");
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::write(&file, page.as_bytes()).unwrap();
    }
    let committed = std::fs::read(&file).unwrap();
    assert!(
        committed == page.as_bytes(),
        "{} differs from the page the fixture renders; run this test with SNAPSHOTS=overwrite",
        file.display()
    );
}

#[test]
fn the_page_fetches_nothing_and_a_closing_tag_in_a_title_cannot_end_a_block() {
    let page = rendered(&fixture("pass.json"), &fixture("facts.json"));
    for fetches in [
        "fetch(",
        "XMLHttpRequest",
        "WebSocket",
        "import(",
        "<link",
        "<img",
        " src=",
    ] {
        assert!(!page.contains(fetches), "the page holds {fetches}");
    }
    assert!(
        !page.contains("href=\"http"),
        "the page links nowhere outside itself"
    );
    let mut facts = fixture("facts.json");
    facts["changes"][0]["title"] = json!("Close </script> early");
    let page = rendered(&fixture("pass.json"), &facts);
    assert!(page.contains("Close <\\/script> early"));
    assert!(!page.contains("Close </script> early"));
}
