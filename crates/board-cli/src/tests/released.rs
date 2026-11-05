//! The released interfaces, listed in released.json at the repository
//! root, change only by a new version (STANDARDS.md 3 and 4): a listed
//! schema keeps its shape and every enum value, a listed vector file keeps
//! its bytes, a listed report keeps every field, and a listed code stays.
//! `SNAPSHOTS=overwrite` lists everything present, which is the step a
//! release takes, and `RELEASED=all` refuses a release that ships something
//! the file does not list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use board_model::{Answer, canonical_json};
use board_ops::{AnswerReport, PutReport};
use board_serve::{Opened, Served};
use serde::{Deserialize, Serialize};

use super::*;
use crate::render::{self, BUDGET, Printer};

const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../released.json");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Where the plugins Board ships keep their schemas, relative to the
/// repository root: `plugins/<name>/types/` for a plugin in a directory and
/// `crates/board-plugin-<name>/types/` for a built-in one. `.board/types/`
/// is left out: it holds the copies `plugin enable` wrote for the
/// repository's own board, which its `controls/` records need, and the
/// crate's copy is the one Board ships.
const SCHEMAS: &[&str] = &["plugins", "crates"];

/// Schemas that change in place until a later release (released.md). The
/// changes plugin is being ported and its records split, so freezing its
/// types now would freeze a shape the port is about to change.
const UNFROZEN: &[&str] = &["plugins/changes/"];

/// Where the repository keeps the vectors other repositories test against.
const VECTORS: &str = "vectors";

/// The crates of the built-in plugins, whose sources name their codes.
const PLUGIN_CRATES: &str = "crates";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Released {
    /// Each schema file by its path from the repository root.
    types: BTreeMap<String, Frozen>,
    /// Each `--json` report by its `schema`, with the paths of the fields
    /// it holds; `[]` stands for every element of an array.
    reports: BTreeMap<String, BTreeSet<String>>,
    /// Board's codes and those of every built-in plugin.
    codes: BTreeSet<String>,
    /// Each vector file but a README, by its path from the repository root,
    /// with the git blob id of its bytes.
    vectors: BTreeMap<String, String>,
}

/// A schema as the freeze holds it: its shape, which may not change, and
/// the values of each enum, which may grow but not shrink (released.md).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frozen {
    /// The git blob id of the schema's canonical JSON with every `enum`
    /// emptied.
    shape: String,
    /// The values of each `enum`, by the JSON pointer of the keyword.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    enums: BTreeMap<String, Vec<Value>>,
}

/// Keywords whose values are instances, not schemas, so an `enum` inside
/// one is data.
const INSTANCES: &[&str] = &["const", "default", "examples", "enum"];

/// The schema `bytes` as the freeze holds it.
fn frozen(hasher: &Scenario, bytes: &[u8]) -> Frozen {
    let mut schema: Value = serde_json::from_slice(bytes).unwrap();
    let mut enums = BTreeMap::new();
    emptied(&mut schema, "", &mut enums);
    let shape = hasher.blob(canonical_json(&schema).as_str()).to_string();
    Frozen { shape, enums }
}

/// Empties every `enum` in `schema`, at `pointer`, into `enums`.
fn emptied(schema: &mut Value, pointer: &str, enums: &mut BTreeMap<String, Vec<Value>>) {
    match schema {
        Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                let at = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                if key == "enum"
                    && let Value::Array(values) = value
                {
                    enums.insert(at, std::mem::take(values));
                } else if !INSTANCES.contains(&key.as_str()) {
                    emptied(value, &at, enums);
                }
            }
        }
        Value::Array(elements) => {
            for (index, element) in elements.iter_mut().enumerate() {
                emptied(element, &format!("{pointer}/{index}"), enums);
            }
        }
        _ => {}
    }
}

/// What a listed entry would have to change in place to be as it is now.
fn violations(released: &Released, present: &Released) -> Vec<String> {
    let mut found = Vec::new();
    for (path, was) in &released.types {
        let Some(now) = present.types.get(path) else {
            found.push(format!("{path} is gone; an old version stays readable"));
            continue;
        };
        if now.shape != was.shape {
            found.push(format!("{path} changed; a changed type is a new version"));
            continue;
        }
        for (pointer, values) in &was.enums {
            let held = now.enums.get(pointer);
            for value in values {
                if !held.is_some_and(|held| held.contains(value)) {
                    found.push(format!(
                        "{path} no longer admits {value} at {pointer}; an enum value is never removed"
                    ));
                }
            }
        }
    }
    for (report, fields) in &released.reports {
        for field in fields {
            let held = present.reports.get(report);
            if !held.is_some_and(|held| held.contains(field)) {
                found.push(format!("{report} no longer holds {field}"));
            }
        }
    }
    for (path, blob) in &released.vectors {
        match present.vectors.get(path) {
            Some(now) if now == blob => {}
            Some(_) => found.push(format!("{path} changed; a changed vector is a new version")),
            None => found.push(format!("{path} is gone; a released vector stays")),
        }
    }
    for code in &released.codes {
        if !present.codes.contains(code) {
            found.push(format!("{code} is gone; a code is never renamed"));
        }
    }
    found
}

/// Fields whose contents are not Board's: a record, a plugin's schemas and
/// metadata, a JSON Schema, and what jsonschema says about a record.
const FOREIGN: &[&str] = &[
    "record",
    "inputSchema",
    "outputSchema",
    "_meta",
    "metadata",
    "failures",
    "schema",
];

/// Every path at which `value` holds something, under `prefix`.
fn paths(prefix: &str, value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let path = format!("{prefix}.{key}");
                if !FOREIGN.contains(&key.as_str()) {
                    paths(&path, value, into);
                }
                into.insert(path);
            }
        }
        Value::Array(elements) => {
            for element in elements {
                paths(&format!("{prefix}[]"), element, into);
            }
        }
        _ => {}
    }
}

/// Every field path the JSON Schema `node` of a document declares, under
/// `prefix`, following `$ref`s into the root's `$defs` once each, `items`
/// as `[]`, and every variant of `oneOf`, `anyOf` and `allOf`. A value the
/// schema leaves open, such as a `data` that Board builds as JSON, adds no
/// path below itself.
fn declared(
    root: &Value,
    node: &Value,
    prefix: &str,
    seen: &BTreeSet<&str>,
    into: &mut BTreeSet<String>,
) {
    let Value::Object(schema) = node else {
        return;
    };
    if let Some(name) = (schema.get("$ref").and_then(Value::as_str))
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        && !seen.contains(name)
        && let Some(definition) = root.get("$defs").and_then(|defs| defs.get(name))
    {
        let mut seen = seen.clone();
        seen.insert(name);
        declared(root, definition, prefix, &seen, into);
    }
    if let Some(Value::Object(properties)) = schema.get("properties") {
        for (key, property) in properties {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            declared(root, property, &path, seen, into);
            into.insert(path);
        }
    }
    if let Some(items) = schema.get("items") {
        declared(root, items, &format!("{prefix}[]"), seen, into);
    }
    for variants in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(variants)) = schema.get(variants) {
            for variant in variants {
                declared(root, variant, prefix, seen, into);
            }
        }
    }
}

/// Every file under `directory` that `wanted` admits by its path from the
/// repository root, with its bytes.
fn files(directory: &Path, wanted: fn(&str) -> bool, into: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, wanted, into);
            continue;
        }
        let relative = path.strip_prefix(ROOT).unwrap().to_string_lossy();
        let relative = relative.replace('\\', "/");
        if wanted(&relative) {
            into.push((relative, std::fs::read(&path).unwrap()));
        }
    }
}

/// Whether the file at `relative` is a schema a plugin Board ships keeps,
/// outside [`UNFROZEN`].
fn is_schema(relative: &str) -> bool {
    let in_plugin = relative.starts_with("plugins/")
        || (relative.starts_with("crates/board-plugin-")
            && relative.split('/').nth(2) == Some("types"));
    relative.ends_with(".schema.json")
        && in_plugin
        && !UNFROZEN
            .iter()
            .any(|unfrozen| relative.starts_with(unfrozen))
}

fn is_vector(relative: &str) -> bool {
    !relative.ends_with("/README.md")
}

/// Whether the file at `relative` is a built-in plugin's source outside
/// its tests.
fn is_plugin_source(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    parts.len() > 3
        && parts[1].starts_with("board-plugin-")
        && parts[2] == "src"
        && Path::new(relative)
            .extension()
            .is_some_and(|extension| extension == "rs")
        && !parts
            .iter()
            .any(|part| *part == "tests" || *part == "tests.rs")
}

/// Each code the source text of the plugin `name` spells: a string literal
/// that is `name` and two or more `::`-separated words, or a format string
/// that starts with a `{CONSTANT}` standing for `name`, as
/// `format!("{PLUGIN}::repo::not_synced")` does.
fn plugin_codes(name: &str, source: &str, into: &mut BTreeSet<String>) {
    for (at, _) in source.match_indices('"') {
        let literal = &source[at + 1..];
        let rest = if let Some(rest) = literal.strip_prefix(name) {
            rest
        } else if let Some(rest) = literal.strip_prefix('{')
            && let Some((constant, rest)) = rest.split_once('}')
            && !constant.is_empty()
            && constant.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        {
            rest
        } else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        let words: Vec<&str> = rest[..end].split("::").collect();
        let word = |word: &&str| {
            !word.is_empty()
                && (word.chars()).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        };
        if words.len() >= 3 && words[0].is_empty() && words[1..].iter().all(word) {
            into.insert(format!("{name}{}", &rest[..end]));
        }
    }
}

/// Codes that Board's own crates build from a plugin's name, as
/// `<plugin>::<family>::<reason>`: each plugin whose source names the
/// marker can fail with them. Each entry names the source that builds the
/// codes, which [`shared_codes`] checks still spells each one.
const SHARED: &[(&str, &str, &[&str])] = &[
    (
        "board_refused(",
        "crates/board-ops/src/host.rs",
        &["board::refused"],
    ),
    (
        "read_input",
        "crates/board-ops/src/host.rs",
        &["call::bad_input"],
    ),
    (
        "Agent::named",
        "crates/board-ops/src/agent.rs",
        &[
            "agent::not_set",
            "agent::invalid",
            "agent::not_started",
            "agent::timeout",
            "agent::failed",
        ],
    ),
    (
        "board_repos::",
        "crates/board-repos/src/listing.rs",
        &["board::refused", "repo::invalid_record", "repo::not_found"],
    ),
];

/// Whether `source` builds `<plugin>::<suffix>`: in one format string, or
/// as `<plugin>::<family>::{reason}` with the reason as its own literal.
fn builds(source: &str, suffix: &str) -> bool {
    let (family, reason) = suffix.split_once("::").unwrap();
    source.contains(&format!("\"{{plugin}}::{suffix}\""))
        || (source.contains(&format!("\"{{plugin}}::{family}::{{reason}}\""))
            && source.contains(&format!("\"{reason}\"")))
}

/// The codes of [`SHARED`] that the plugin `name`, whose sources are
/// `sources`, can fail with.
fn shared_codes(name: &str, sources: &[&str], into: &mut BTreeSet<String>) {
    for (marker, builder, suffixes) in SHARED {
        if !sources.iter().any(|source| source.contains(marker)) {
            continue;
        }
        let built = std::fs::read_to_string(Path::new(ROOT).join(builder)).unwrap();
        for suffix in *suffixes {
            assert!(
                builds(&built, suffix),
                "{builder} no longer builds <plugin>::{suffix}"
            );
            into.insert(format!("{name}::{suffix}"));
        }
    }
}

/// Every code a built-in plugin's source names or [`SHARED`] gives it.
fn plugins_codes() -> BTreeSet<String> {
    let mut sources = Vec::new();
    files(
        &Path::new(ROOT).join(PLUGIN_CRATES),
        is_plugin_source,
        &mut sources,
    );
    let mut by_plugin: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (relative, bytes) in sources {
        let krate = relative.split('/').nth(1).unwrap();
        let name = krate.strip_prefix("board-plugin-").unwrap().to_owned();
        let text = String::from_utf8(bytes).unwrap();
        by_plugin.entry(name).or_default().push(text);
    }
    let mut codes = BTreeSet::new();
    for (name, texts) in &by_plugin {
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        for text in &texts {
            plugin_codes(name, text, &mut codes);
        }
        shared_codes(name, &texts, &mut codes);
    }
    codes
}

/// Every document `board schemas list` names, with the fields its JSON
/// Schema declares. Run outside a board, it lists Board's own documents.
fn documents() -> BTreeMap<String, BTreeSet<String>> {
    let directory = Scenario::new();
    let listed = board_in(
        directory.workdir(),
        &["--json", "schemas", "list", "--full", "--all"],
    );
    assert_eq!(listed.code, 0, "{}", text(&listed.stderr));
    let ids: Vec<String> = (json(&listed)["data"].as_array().unwrap().iter())
        .map(|row| row["id"].as_str().unwrap().to_owned())
        .collect();
    let mut arguments = vec!["--json", "schemas", "show"];
    arguments.extend(ids.iter().map(String::as_str));
    let shown = board_in(directory.workdir(), &arguments);
    assert_eq!(shown.code, 0, "{}", text(&shown.stderr));
    let mut documents = BTreeMap::new();
    for row in json(&shown)["data"].as_array().unwrap() {
        let mut fields = BTreeSet::new();
        declared(
            &row["schema"],
            &row["schema"],
            "",
            &BTreeSet::new(),
            &mut fields,
        );
        documents.insert(row["id"].as_str().unwrap().to_owned(), fields);
    }
    assert_eq!(documents.len(), ids.len(), "schemas show left out an id");
    documents
}

/// Reports whose `data` is not Board's to freeze: `query`'s is the
/// statement's rows and `call`'s is the plugin's output. Their envelopes
/// are frozen as their JSON Schemas declare them.
const OPEN_DATA: &[&str] = &["board/query-report/v2", "board/call-report/v2"];

/// Every report once, from a board holding a record, a mount one commit
/// behind its library, and the tasks plugin shipping a schema that
/// differs from the board's copy. The put lands a degraded record, so the
/// check and the refs report a code. A second clone's sync replays a
/// commit, so its report holds `replayed`. `serve`, `ui open` and `answer`
/// need a server, a browser and the owner's signer, so their reports come
/// from the functions that render them.
fn samples() -> Vec<(String, Value)> {
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    library.commit(&[("a.md", "two")]);
    let (board, data) = plugins::holding(r#"{"type": "array"}"#);
    board
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        )
        .write(
            ".board/types/lr/project/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        )
        .write(
            "tasks/a.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
        )
        .write(
            "vendor/lib.json",
            Item::File(&mount_of(&library.path(), old)),
        );
    let plugins = data.workdir().join("plugins");
    let record = br#"{"$type": "lr/task/v1", "title": "b", "project": "/missing"}"#;
    let runs: &[(&[&str], &[u8])] = &[
        (&["init"], b""),
        (&["put", "/tasks/b"], record),
        (&["check"], b""),
        (&["get", "/projects/p", "/tasks/a"], b""),
        (&["ls", "/"], b""),
        (&["refs", "/tasks/a", "/tasks/b"], b""),
        (
            &["decide", "--op", "review.comment", "--path", "/tasks/a"],
            b"",
        ),
        (&["mount", "update", "/vendor/lib"], b""),
        (&["plugin", "list"], b""),
        (&["plugin", "find"], b""),
        (&["plugin", "enable", "tasks"], b""),
        (&["type", "add", "lr/note/v1"], br#"{"type": "object"}"#),
        (&["ui", "template", "record"], b""),
        (&["rm", "/tasks/a"], b""),
    ];
    let mut outputs: Vec<(Vec<&str>, Output)> = runs
        .iter()
        .map(|(arguments, stdin)| {
            let arguments = [&["--json"], *arguments].concat();
            let output = board_full(board.workdir(), None, Some(&plugins), &arguments, stdin);
            (arguments, output)
        })
        .collect();
    let (_remote, a, b) = sync::clones();
    let project = br#"{"$type": "lr/project/v1"}"#;
    for (clone, at) in [(&a, "/projects/a"), (&b, "/projects/b")] {
        board_with(clone.workdir(), None, &["put", at], project);
    }
    board_in(a.workdir(), &["sync"]);
    let arguments = vec!["--json", "sync"];
    outputs.push((arguments.clone(), board_in(b.workdir(), &arguments)));
    let printer = |words: &[&str]| Printer::of(true, BUDGET, words);
    let served = Served {
        port: 1,
        requests: 1,
        idle: std::time::Duration::from_secs(1),
    };
    outputs.push((vec!["serve"], render::serve(printer(&["serve"]), &served)));
    let opened = Opened {
        binding: "/bindings/review".parse().unwrap(),
        port: 1,
        started: true,
    };
    let open = render::ui_open(printer(&["ui", "open"]), &opened);
    outputs.push((vec!["ui", "open"], open));
    let answered = AnswerReport {
        id: "a".repeat(64),
        answer: Answer::Refuse,
        put: PutReport {
            writes: Vec::new(),
            removed: Vec::new(),
            absent: Vec::new(),
            commit: None,
        },
    };
    let answer = render::answer(printer(&["answer"]), &answered);
    outputs.push((vec!["answer"], answer));
    (outputs.into_iter())
        .map(|(arguments, output)| {
            let report = json(&output);
            let schema = report["schema"].as_str().unwrap_or_else(|| {
                panic!("{arguments:?} printed no report: {}", text(&output.stderr))
            });
            (schema.to_owned(), report)
        })
        .collect()
}

/// Every report, with the fields its JSON Schema declares and those its
/// samples hold.
fn reports() -> BTreeMap<String, BTreeSet<String>> {
    let mut reports = documents();
    for (schema, report) in samples() {
        let fields = (reports.get_mut(&schema))
            .unwrap_or_else(|| panic!("{schema} is printed but schemas list does not name it"));
        paths("data", &report["data"], fields);
    }
    reports
}

fn present() -> Released {
    let hasher = Scenario::new();
    let mut schemas = Vec::new();
    for directory in SCHEMAS {
        files(&Path::new(ROOT).join(directory), is_schema, &mut schemas);
    }
    let types = (schemas.into_iter())
        .map(|(path, bytes)| (path, frozen(&hasher, &bytes)))
        .collect();
    let mut vectors = Vec::new();
    files(&Path::new(ROOT).join(VECTORS), is_vector, &mut vectors);
    let vectors = (vectors.into_iter())
        .map(|(path, bytes)| (path, hasher.blob_of(&bytes).to_string()))
        .collect();
    let mut codes = plugins_codes();
    codes.extend(Code::ALL.iter().map(|code| code.as_str().to_owned()));
    Released {
        types,
        reports: reports(),
        codes,
        vectors,
    }
}

/// The mode the environment asks for: `SNAPSHOTS=overwrite` writes the
/// file, as crumbkit-test writes its goldens, and `RELEASED=all` is the
/// release's check that the file lists everything the checkout ships.
#[expect(
    clippy::disallowed_methods,
    reason = "released.json is a golden file, written and selected as crumbkit-test's are"
)]
fn mode() -> (bool, bool) {
    (
        std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite"),
        std::env::var("RELEASED").as_deref() == Ok("all"),
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "released.json is a golden file, written as crumbkit-test's are"
)]
fn write(released: &Released) {
    let text = serde_json::to_string_pretty(released).unwrap();
    std::fs::write(FILE, format!("{text}\n")).unwrap();
}

/// What `SNAPSHOTS=overwrite` may write: everything present, unless an
/// entry `released` lists changed in place, which it refuses as the check
/// does, so an overwrite never freezes an edit.
fn overwritten(released: &Released, present: Released) -> Result<Released, Vec<String>> {
    let found = violations(released, &present);
    if found.is_empty() {
        Ok(present)
    } else {
        Err(found)
    }
}

#[test]
fn every_released_type_report_and_code_is_as_it_was_released() {
    let present = present();
    let (overwrite, all) = mode();
    let released: Released = serde_json::from_slice(&std::fs::read(FILE).unwrap()).unwrap();
    let present = overwritten(&released, present).unwrap_or_else(|found| {
        panic!(
            "released.json lists an interface that changed in place:\n{}",
            found.join("\n")
        )
    });
    // What a release would list reaches every plugin's types and codes and
    // every report, and leaves the changes plugin's types out.
    let built_in = "crates/board-plugin-controls/types/controls/control/v1.schema.json";
    assert!(
        present.types.contains_key(built_in),
        "{:?}",
        present.types.keys()
    );
    assert!(
        !(present.types.keys())
            .any(|path| path.starts_with("plugins/changes/") || path.starts_with(".board/")),
        "{:?}",
        present.types.keys()
    );
    for code in [
        "review::agent::failed",
        "runs::suite::not_found",
        "tasks::repo::sync_timeout",
        "share::target::differs",
        "notes::repo::not_synced",
        "notes::repo::not_found",
        "tasks::call::bad_input",
        "board::path::not_found",
    ] {
        assert!(present.codes.contains(code), "{code}");
    }
    for (report, fields) in &present.reports {
        let sampled = fields
            .iter()
            .any(|field| field.starts_with("data.") || field.starts_with("data[]"));
        let expected = !OPEN_DATA.contains(&report.as_str()) && report != "board/failure/v1";
        assert_eq!(sampled, expected, "{report}: {fields:?}");
    }
    if overwrite {
        write(&present);
        return;
    }
    if all {
        assert_eq!(
            released, present,
            "released.json does not list what this release ships; run this test with SNAPSHOTS=overwrite"
        );
    }
}

/// A schema with a top-level enum and one under a property.
fn schema(status: &[&str], kind: &[&str], description: &str) -> String {
    json!({
        "description": description,
        "enum": [{"status": "open"}, {"status": "done"}],
        "properties": {
            "status": {"enum": status},
            "kind": {"enum": kind, "default": "a"},
            "note": {"const": "x", "examples": [{"enum": ["data"]}]},
        },
    })
    .to_string()
}

#[test]
fn an_enum_that_gains_a_value_is_admitted_and_one_that_loses_one_or_a_changed_shape_is_refused() {
    let hasher = Scenario::new();
    let listed = |schema: &str| Released {
        types: [(
            "crates/board-plugin-x/types/x/y/v1.schema.json".to_owned(),
            frozen(&hasher, schema.as_bytes()),
        )]
        .into(),
        ..Released::default()
    };
    let released = listed(&schema(&["open", "done"], &["a"], "one"));
    assert_eq!(
        released
            .types
            .values()
            .next()
            .unwrap()
            .enums
            .keys()
            .collect::<Vec<_>>(),
        ["/enum", "/properties/kind/enum", "/properties/status/enum"],
        "an enum inside examples is data, not a keyword"
    );
    let gained = listed(&schema(&["done", "blocked", "open"], &["a", "b"], "one"));
    assert_eq!(violations(&released, &gained), Vec::<String>::new());
    assert_eq!(
        overwritten(&released, gained.clone()),
        Ok(gained),
        "an overwrite lists the added values"
    );
    let lost = listed(&schema(&["open"], &["b"], "one"));
    assert_eq!(
        violations(&released, &lost),
        [
            "crates/board-plugin-x/types/x/y/v1.schema.json no longer admits \"a\" at /properties/kind/enum; an enum value is never removed",
            "crates/board-plugin-x/types/x/y/v1.schema.json no longer admits \"done\" at /properties/status/enum; an enum value is never removed",
        ]
    );
    let described = listed(&schema(&["open", "done"], &["a"], "two"));
    assert_eq!(
        violations(&released, &described),
        ["crates/board-plugin-x/types/x/y/v1.schema.json changed; a changed type is a new version"]
    );
    assert!(overwritten(&released, described).is_err());
}

#[test]
fn a_plugin_source_names_its_codes_in_literals_and_in_format_strings_that_start_with_its_name() {
    let source = r#"
        refusal("runs::suite::not_found", message);
        let code = format!("{PLUGIN}::repo::not_synced");
        let other = "board::path::not_found";
        let short = "runs::suite";
        let spaced = "runs::suite not found";
        let lowered = format!("{plugin}::repo::x");
        let nested = "runs::a::b::c_2";
    "#;
    let mut codes = BTreeSet::new();
    plugin_codes("runs", source, &mut codes);
    assert_eq!(
        codes.into_iter().collect::<Vec<_>>(),
        [
            "runs::a::b::c_2",
            "runs::repo::not_synced",
            "runs::suite::not_found"
        ]
    );
}

#[test]
fn a_shared_code_is_built_by_its_source_in_one_string_or_from_its_family_and_reason() {
    let whole = r#"code: format!("{plugin}::board::refused"),"#;
    assert!(builds(whole, "board::refused"));
    assert!(!builds(whole, "board::other"));
    let split = r#"refused(plugin, "timeout", ...); code: format!("{plugin}::agent::{reason}"),"#;
    assert!(builds(split, "agent::timeout"));
    assert!(!builds(split, "agent::failed"));
}

#[test]
fn a_documents_schema_declares_its_fields_through_refs_items_and_variants_once_each() {
    let schema = json!({
        "$defs": {
            "Row": {"properties": {"name": {"type": "string"}, "child": {"$ref": "#/$defs/Row"}}},
            "Open": {"description": "a value Board builds as JSON"},
        },
        "properties": {
            "schema": {"const": "x/y/v1"},
            "data": {"items": {"$ref": "#/$defs/Row"}},
            "said": {"$ref": "#/$defs/Open"},
            "recovery": {"oneOf": [
                {"properties": {"kind": {"const": "run"}, "call": {}}},
                {"properties": {"kind": {"const": "hint"}, "text": {}}},
            ]},
        },
    });
    let mut fields = BTreeSet::new();
    declared(&schema, &schema, "", &BTreeSet::new(), &mut fields);
    assert_eq!(
        fields.into_iter().collect::<Vec<_>>(),
        [
            "data",
            "data[].child",
            "data[].name",
            "recovery",
            "recovery.call",
            "recovery.kind",
            "recovery.text",
            "said",
            "schema",
        ]
    );
}

#[test]
fn only_a_shipped_plugins_schemas_and_sources_are_read_and_the_changes_types_are_left_out() {
    assert!(is_schema(
        "crates/board-plugin-runs/types/runs/run/v1.schema.json"
    ));
    assert!(!is_schema(
        "crates/board-plugin-runs/src/fixture.schema.json"
    ));
    assert!(is_schema("plugins/effects/types/effects/x/v1.schema.json"));
    assert!(!is_schema(
        "plugins/changes/types/changes/change/v1.schema.json"
    ));
    assert!(!is_schema(".board/types/controls/control/v1.schema.json"));
    assert!(is_plugin_source(
        "crates/board-plugin-runs/src/qualifying.rs"
    ));
    assert!(!is_plugin_source(
        "crates/board-plugin-runs/src/qualifying/tests.rs"
    ));
    assert!(!is_plugin_source("crates/board-plugin-runs/src/tests.rs"));
    assert!(!is_plugin_source("crates/board-ops/src/check.rs"));
}

#[test]
fn a_changed_schema_a_dropped_field_and_a_gone_code_are_refused_and_the_unchanged_are_admitted() {
    let hasher = Scenario::new();
    let (one, two) = (frozen(&hasher, b"{}"), frozen(&hasher, b"[]"));
    let listed = |frozen: &Frozen, field: &str, code: &str| Released {
        types: [(
            "plugins/x/types/x/y/v1.schema.json".to_owned(),
            frozen.clone(),
        )]
        .into(),
        reports: [("board/x-report/v2".to_owned(), [field.to_owned()].into())].into(),
        codes: [code.to_owned()].into(),
        vectors: BTreeMap::new(),
    };
    let now = listed(&one, "data.kept", "board::x::kept");
    assert_eq!(violations(&now, &now), Vec::<String>::new());
    let changed = listed(&two, "data.gone", "board::x::gone");
    assert_eq!(
        violations(&changed, &now),
        [
            "plugins/x/types/x/y/v1.schema.json changed; a changed type is a new version",
            "board/x-report/v2 no longer holds data.gone",
            "board::x::gone is gone; a code is never renamed",
        ]
    );
    let gone = Released {
        types: [("plugins/x/types/x/z/v1.schema.json".to_owned(), one)].into(),
        ..Released::default()
    };
    assert_eq!(
        violations(&gone, &now),
        ["plugins/x/types/x/z/v1.schema.json is gone; an old version stays readable"]
    );
}

#[test]
fn a_changed_or_gone_vector_is_refused_and_an_unchanged_one_is_admitted() {
    let hasher = Scenario::new();
    let (one, two) = (hasher.blob("a").to_string(), hasher.blob("b").to_string());
    let listed = |path: &str, blob: &str| Released {
        vectors: [(path.to_owned(), blob.to_owned())].into(),
        ..Released::default()
    };
    let now = listed("vectors/signing/v1/approval.sig", &one);
    assert_eq!(violations(&now, &now), Vec::<String>::new());
    assert_eq!(
        violations(&listed("vectors/signing/v1/approval.sig", &two), &now),
        ["vectors/signing/v1/approval.sig changed; a changed vector is a new version"]
    );
    assert_eq!(
        violations(&listed("vectors/signing/v1/write.sig", &one), &now),
        ["vectors/signing/v1/write.sig is gone; a released vector stays"]
    );
}

#[test]
fn an_overwrite_that_would_change_a_released_vector_is_refused_and_one_that_adds_a_vector_is_admitted()
 {
    let hasher = Scenario::new();
    let (one, two) = (hasher.blob("a").to_string(), hasher.blob("b").to_string());
    let listed = |entries: &[(&str, &str)]| Released {
        vectors: entries
            .iter()
            .map(|(path, blob)| ((*path).to_owned(), (*blob).to_owned()))
            .collect(),
        ..Released::default()
    };
    let released = listed(&[("vectors/signing/v1/approval.sig", &one)]);
    let added = listed(&[
        ("vectors/signing/v1/approval.sig", &one),
        ("vectors/signing/v2/approval.sig", &two),
    ]);
    assert_eq!(overwritten(&released, added.clone()), Ok(added));
    let edited = listed(&[("vectors/signing/v1/approval.sig", &two)]);
    assert_eq!(
        overwritten(&released, edited),
        Err(vec![
            "vectors/signing/v1/approval.sig changed; a changed vector is a new version".to_owned()
        ])
    );
}
