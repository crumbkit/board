//! Every record schema committed in the repository keeps the shape rules
//! that a reading of the schema can check
//! (docs/architecture/records/shapes.md). The rules on naming, enums and
//! `$defs` need judgement and are left to review.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The rules a schema may break today, by record type, until the plugin
/// that ships it is ported. Each entry must still be broken, so a fix
/// removes its entry.
const EXEMPT: [(&str, &str); 25] = [
    // Python plugins, and controls, which ships the pydantic-generated
    // schema file unchanged: pydantic titles every property and keeps field
    // meanings in the class docstring.
    ("changes/change/v1", "described"),
    ("changes/change/v1", "untitled"),
    ("controls/control/v1", "described"),
    ("controls/control/v1", "untitled"),
    ("notes/repo/v1", "described"),
    ("notes/repo/v1", "untitled"),
    ("prompts/prompt/v1", "described"),
    ("prompts/prompt/v1", "untitled"),
    ("prompts/repos/v1", "described"),
    ("prompts/repos/v1", "untitled"),
    ("review/brief/v1", "described"),
    ("review/brief/v1", "untitled"),
    ("review/report/v1", "described"),
    ("review/report/v1", "untitled"),
    ("runs/run/v1", "described"),
    ("runs/run/v1", "untitled"),
    ("runs/suite/v1", "described"),
    ("runs/suite/v1", "untitled"),
    ("tasks/task/v2", "described"),
    ("tasks/task/v2", "untitled"),
    // `rule` admits null, though no stored control holds one.
    ("controls/control/v1", "absent-not-null"),
    // Another tool co-writes tasks, so a task keeps fields it does not know.
    ("tasks/task/v2", "closed"),
    // This repository's board's copy of the controls schema.
    (".board/controls/control/v1", "described"),
    (".board/controls/control/v1", "untitled"),
    (".board/controls/control/v1", "absent-not-null"),
];

/// The repository's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every `*.schema.json` under a `types/` directory of a plugin, a crate or
/// the repository's own board, by the name its rules are exempted under.
#[cfg(test)]
fn schemas() -> Vec<(String, Value)> {
    let root = root();
    let mut found = Vec::new();
    let mut tops = vec![root.join(".board/types")];
    for parent in ["plugins", "crates"] {
        for entry in fs::read_dir(root.join(parent)).unwrap() {
            tops.push(entry.unwrap().path().join("types"));
        }
    }
    for top in tops {
        let mut pending = vec![top.clone()];
        while let Some(directory) = pending.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let Some(name) = path
                    .to_str()
                    .and_then(|path| path.strip_suffix(".schema.json"))
                else {
                    pending.push(path);
                    continue;
                };
                let mut name = Path::new(name)
                    .strip_prefix(&top)
                    .unwrap()
                    .display()
                    .to_string();
                if top.starts_with(root.join(".board")) {
                    name = format!(".board/{name}");
                }
                let text = fs::read(&path).unwrap();
                found.push((name, serde_json::from_slice(&text).unwrap()));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The rules `schema` breaks, each with where.
#[cfg(test)]
fn broken(schema: &Value) -> Vec<(&'static str, String)> {
    let mut broken = Vec::new();
    if let Err(error) = board_validate::Schema::compile(schema.clone()) {
        broken.push(("compiles", error.to_string()));
    }
    if schema.get("$schema").is_some() {
        broken.push(("no-schema-key", String::new()));
    }
    let tag = &schema["properties"]["$type"];
    let required = schema["required"].as_array().cloned().unwrap_or_default();
    if tag["type"] != "string" || !tag["const"].is_string() || !required.contains(&"$type".into()) {
        broken.push(("typed", tag.to_string()));
    }
    objects(schema, "", &mut broken);
    broken
}

/// Checks each object schema in `node`, which sits at `at`.
#[cfg(test)]
fn objects(node: &Value, at: &str, broken: &mut Vec<(&'static str, String)>) {
    match node {
        Value::Object(keywords) => {
            if keywords.get("type").and_then(Value::as_str) == Some("object") {
                object(keywords, at, broken);
            }
            for (key, value) in keywords {
                objects(value, &format!("{at}/{key}"), broken);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                objects(value, &format!("{at}/{index}"), broken);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
fn object(
    keywords: &serde_json::Map<String, Value>,
    at: &str,
    broken: &mut Vec<(&'static str, String)>,
) {
    let properties = keywords.get("properties").and_then(Value::as_object);
    let map = properties.is_none()
        && keywords
            .get("additionalProperties")
            .is_some_and(Value::is_object);
    if !map && keywords.get("additionalProperties") != Some(&Value::Bool(false)) {
        broken.push(("closed", at.to_owned()));
    }
    let required: BTreeSet<&str> = (keywords
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(Value::as_str)
    .collect();
    for (name, property) in properties.into_iter().flatten() {
        let place = format!("{at}/properties/{name}");
        if property.get("description").is_none() {
            broken.push(("described", place.clone()));
        }
        if property.get("title").is_some() {
            broken.push(("untitled", place.clone()));
        }
        let null = json!({"type": "null"});
        let nullable = property["type"] == "null"
            || property["type"]
                .as_array()
                .is_some_and(|types| types.contains(&"null".into()))
            || property["anyOf"]
                .as_array()
                .is_some_and(|branches| branches.contains(&null));
        if nullable && !required.contains(name.as_str()) {
            broken.push(("absent-not-null", place));
        }
    }
}

#[test]
fn every_committed_record_schema_keeps_the_shape_rules_or_is_listed_as_exempt() {
    let schemas = schemas();
    assert!(
        schemas.iter().any(|(name, _)| name == "plans/question/v1"),
        "{schemas:?}"
    );
    let mut used = BTreeSet::new();
    let mut failures = Vec::new();
    for (name, schema) in &schemas {
        for (rule, place) in broken(schema) {
            if EXEMPT.contains(&(name.as_str(), rule)) {
                used.insert((name.as_str(), rule));
            } else {
                failures.push(format!("{name} breaks {rule} at {place:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    let unused: Vec<_> = EXEMPT
        .iter()
        .filter(|entry| !used.contains(entry))
        .collect();
    assert!(
        unused.is_empty(),
        "these schemas now keep the rule; remove their entries: {unused:?}"
    );
}

#[test]
fn each_rule_the_test_checks_is_found_broken_in_a_schema_and_kept_in_its_twin() {
    let kept = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["$type"],
        "properties": {
            "$type": {"type": "string", "const": "a/b/v1", "description": "Type."},
            "labels": {
                "description": "A map.",
                "type": "object",
                "additionalProperties": {"type": "string"}
            },
            "r": {"type": "string", "format": "board-ref", "description": "Ref."}
        }
    });
    assert_eq!(broken(&kept), []);
    let hidden =
        json!({"description": "Ref.", "anyOf": [{"type": "string", "format": "board-ref"}]});
    let changes: [(&str, &str, Value); 7] = [
        ("compiles", "/properties/r", hidden),
        (
            "no-schema-key",
            "/$schema",
            json!("https://json-schema.org/draft/2020-12/schema"),
        ),
        ("typed", "/required", json!([])),
        ("closed", "/additionalProperties", json!(true)),
        ("described", "/properties/r/description", Value::Null),
        ("untitled", "/properties/r/title", json!("R")),
        (
            "absent-not-null",
            "/properties/r/type",
            json!(["string", "null"]),
        ),
    ];
    for (rule, pointer, value) in changes {
        let mut schema = kept.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let parent = schema.pointer_mut(parent).unwrap().as_object_mut().unwrap();
        if value.is_null() {
            parent.remove(key);
        } else {
            parent.insert(key.to_owned(), value);
        }
        let rules: Vec<&str> = broken(&schema).into_iter().map(|(rule, _)| rule).collect();
        assert_eq!(rules, [rule]);
    }
}
