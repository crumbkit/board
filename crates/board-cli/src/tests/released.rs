//! The released interfaces, listed in released.json at the repository
//! root, change only by a new version (STANDARDS.md 3 and 4): a listed
//! schema keeps its shape and every enum value, a listed vector file keeps
//! its bytes, a listed report keeps every field, and a listed code stays.
//! `SNAPSHOTS=overwrite` lists everything present, which is the step a
//! release takes, and `RELEASED=all` refuses a release that ships something
//! the file does not list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use board_model::Location;
use board_model::{Answer, canonical_json};
use board_ops::{AnswerReport, Detail, EnableReport, Kept, PutReport, Removed, Written};
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
    /// Each built-in plugin's capability, as `<plugin>.<capability>`, with
    /// the fields its input schema declares, under `input`, and its output
    /// schema, under `output`, each by its path.
    #[serde(default)]
    capabilities: BTreeMap<String, BTreeMap<String, Field>>,
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

/// A capability's field as the freeze holds it (released.md): its types may
/// not change, an input may not become required, and its enum values may
/// grow but not shrink.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    /// The JSON Schema types the field's schema names; empty when it names
    /// none.
    #[serde(default, rename = "type", skip_serializing_if = "BTreeSet::is_empty")]
    types: BTreeSet<String>,
    /// The fields a value must hold, one set for each way to satisfy its
    /// schema; empty when it requires none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    requires: Vec<BTreeSet<String>>,
    /// The values the field's `enum` or `const` admits.
    #[serde(default, rename = "enum", skip_serializing_if = "Vec::is_empty")]
    values: Vec<Value>,
}

/// Keywords whose values are instances, not schemas, so an `enum` inside
/// one is data.
const INSTANCES: &[&str] = &["const", "default", "examples", "enum"];

/// Keywords under which an added enum value can refuse a record the schema
/// admitted: it changes which branch of an `if` applies, makes a `not`
/// refuse more, or lets a value match two branches of a `oneOf`. An enum
/// under one of them stays in the shape, so adding to it is a new version.
const NARROWING: &[&str] = &["if", "not", "oneOf"];

/// The schema `bytes` as the freeze holds it.
fn frozen(hasher: &Scenario, bytes: &[u8]) -> Frozen {
    let mut schema: Value = serde_json::from_slice(bytes).unwrap();
    let mut enums = BTreeMap::new();
    let kept = narrowed(&schema);
    if !kept.contains("") {
        emptied(&mut schema, "", &kept, &mut enums);
    }
    let shape = hasher.blob(canonical_json(&schema).as_str()).to_string();
    Frozen { shape, enums }
}

/// The JSON pointers of the subschemas that a `$ref` under a [`NARROWING`]
/// keyword reaches, and those that their own `$ref`s reach.
fn narrowed(schema: &Value) -> BTreeSet<String> {
    let (mut kept, mut pending) = (BTreeSet::new(), Vec::new());
    references(schema, false, &mut pending);
    while let Some(pointer) = pending.pop() {
        if let Some(node) = schema.pointer(&pointer)
            && kept.insert(pointer)
        {
            references(node, true, &mut pending);
        }
    }
    kept
}

/// The pointers of the local `$ref`s in `node` that sit under a
/// [`NARROWING`] keyword, or all of them when `under` is set.
fn references(node: &Value, under: bool, into: &mut Vec<String>) {
    match node {
        Value::Object(fields) => {
            for (key, value) in fields {
                if key == "$ref" {
                    if let Some(pointer) = value.as_str().and_then(|r| r.strip_prefix('#'))
                        && under
                    {
                        into.push(pointer.to_owned());
                    }
                } else if !INSTANCES.contains(&key.as_str()) {
                    let under = under || NARROWING.contains(&key.as_str());
                    references(value, under, into);
                }
            }
        }
        Value::Array(elements) => {
            for element in elements {
                references(element, under, into);
            }
        }
        _ => {}
    }
}

/// Empties every `enum` in `schema`, at `pointer`, into `enums`, but none
/// under a [`NARROWING`] keyword or in a subschema at a pointer in `kept`.
fn emptied(
    schema: &mut Value,
    pointer: &str,
    kept: &BTreeSet<String>,
    enums: &mut BTreeMap<String, Vec<Value>>,
) {
    match schema {
        Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                let at = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                if key == "enum"
                    && let Value::Array(values) = value
                {
                    enums.insert(at, std::mem::take(values));
                } else if !INSTANCES.contains(&key.as_str())
                    && !NARROWING.contains(&key.as_str())
                    && !kept.contains(&at)
                {
                    emptied(value, &at, kept, enums);
                }
            }
        }
        Value::Array(elements) => {
            for (index, element) in elements.iter_mut().enumerate() {
                let at = format!("{pointer}/{index}");
                if !kept.contains(&at) {
                    emptied(element, &at, kept, enums);
                }
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
            if !(present.reports.get(report)).is_some_and(|held| held.contains(field)) {
                found.push(format!("{report} no longer holds {field}"));
            }
        }
    }
    for (capability, fields) in &released.capabilities {
        let Some(held) = present.capabilities.get(capability) else {
            found.extend(
                fields
                    .keys()
                    .map(|path| format!("{capability} no longer holds {path}")),
            );
            continue;
        };
        found.extend(changed(capability, fields, held));
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

/// What the capability `name`, which declared `was`, broke by declaring
/// `now`: a field gone, a type changed, an enum value removed, an input
/// newly limited to an enum, an input newly required or an output field no
/// longer always held. Only listed fields are compared, since a new input
/// that only a new optional input requires breaks no caller.
fn changed(
    name: &str,
    was: &BTreeMap<String, Field>,
    now: &BTreeMap<String, Field>,
) -> Vec<String> {
    let mut found = Vec::new();
    let types = |field: &Field| {
        (field.types.iter().cloned())
            .collect::<Vec<_>>()
            .join(" or ")
    };
    for (path, field) in was {
        let Some(held) = now.get(path) else {
            found.push(format!("{name} no longer holds {path}"));
            continue;
        };
        if path == "input" || path.starts_with("input.") {
            found.extend(required(name, path, &field.requires, &held.requires));
            if field.values.is_empty() && !held.values.is_empty() {
                found.push(format!(
                    "{name} now limits {path} to {}; an enum on an input is a new version",
                    Value::from(held.values.clone())
                ));
            }
        } else {
            let (before, after) = (always(&field.requires), always(&held.requires));
            for child in before.difference(&after) {
                found.push(format!(
                    "{name} no longer always holds {path}.{child}; a reader relies on it"
                ));
            }
        }
        for value in (field.values.iter()).filter(|value| !held.values.contains(value)) {
            found.push(format!(
                "{name} no longer admits {value} at {path}; an enum value is never removed"
            ));
        }
        if held.types != field.types {
            found.push(format!(
                "{name} changed the type of {path} from {} to {}; a changed type is a new version",
                types(field),
                types(held)
            ));
        }
    }
    found
}

/// The ways to satisfy a schema whose `requires` is `ways`: one that requires
/// nothing when `ways` is empty.
fn ways(ways: &[BTreeSet<String>]) -> Vec<BTreeSet<String>> {
    if ways.is_empty() {
        vec![BTreeSet::new()]
    } else {
        ways.to_vec()
    }
}

/// The fields every way to satisfy a schema whose `requires` is `ways`
/// holds.
fn always(ways: &[BTreeSet<String>]) -> BTreeSet<String> {
    let mut all = self::ways(ways).into_iter();
    let first = all.next().unwrap_or_default();
    all.fold(first, |always, way| &always & &way)
}

/// The inputs of the field `path` of `name` that a caller who satisfied
/// `was` must now add to satisfy `now`: for each old way that no new way
/// fits inside, the fields the closest new way adds.
fn required(
    name: &str,
    path: &str,
    was: &[BTreeSet<String>],
    now: &[BTreeSet<String>],
) -> Vec<String> {
    let now = ways(now);
    let mut added = BTreeSet::new();
    for way in ways(was) {
        if !now.iter().any(|new| new.is_subset(&way)) {
            let closest = now.iter().map(|new| new - &way).min_by_key(BTreeSet::len);
            added.extend(closest.unwrap_or_default());
        }
    }
    (added.into_iter())
        .map(|child| {
            format!("{name} now requires {path}.{child}; a newly required input is a new version")
        })
        .collect()
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
    into: &mut BTreeMap<String, Field>,
) {
    let Value::Object(schema) = node else {
        return;
    };
    if let Some(field) = into.get_mut(prefix) {
        match schema.get("type") {
            Some(Value::String(kind)) => {
                field.types.insert(kind.clone());
            }
            Some(Value::Array(kinds)) => {
                let kinds = kinds.iter().filter_map(Value::as_str);
                field.types.extend(kinds.map(str::to_owned));
            }
            _ => {}
        }
        let one = schema.get("const").map(std::slice::from_ref);
        let values = (schema.get("enum").and_then(Value::as_array)).map(Vec::as_slice);
        for value in one.into_iter().chain(values).flatten() {
            if !field.values.contains(value) {
                field.values.push(value.clone());
            }
        }
    }
    if let Some((definition, seen)) = referenced(root, schema, seen) {
        declared(root, definition, prefix, &seen, into);
    }
    if let Some(Value::Object(properties)) = schema.get("properties") {
        for (key, property) in properties {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            entered(root, property, &path, seen, into);
        }
    }
    if let Some(values @ Value::Object(_)) = schema.get("additionalProperties") {
        entered(root, values, &format!("{prefix}.*"), seen, into);
    }
    if let Some(items) = schema.get("items") {
        entered(root, items, &format!("{prefix}[]"), seen, into);
    }
    for variants in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(variants)) = schema.get(variants) {
            for variant in variants {
                declared(root, variant, prefix, seen, into);
            }
        }
    }
}

/// The definition the local `$ref` of `schema` names, with `seen` and its
/// name, unless `seen` already holds it.
fn referenced<'a>(
    root: &'a Value,
    schema: &'a serde_json::Map<String, Value>,
    seen: &BTreeSet<&'a str>,
) -> Option<(&'a Value, BTreeSet<&'a str>)> {
    let name = (schema.get("$ref").and_then(Value::as_str))?.strip_prefix("#/$defs/")?;
    let definition = root.get("$defs")?.get(name)?;
    let mut seen = seen.clone();
    seen.insert(name).then_some((definition, seen))
}

/// Declares the field at `path`, whose schema is `node`, with each way to
/// satisfy it, and every field below it.
fn entered(
    root: &Value,
    node: &Value,
    path: &str,
    seen: &BTreeSet<&str>,
    into: &mut BTreeMap<String, Field>,
) {
    let field = into.entry(path.to_owned()).or_default();
    let mut ways = std::mem::take(&mut field.requires);
    ways.extend(requirements(root, node, seen));
    field.requires = least(ways);
    declared(root, node, path, seen, into);
}

/// The sets of fields a value of `node` must hold, one for each way to
/// satisfy it: its `required` with those of its `$ref` and every `allOf`
/// variant, and with those of one `oneOf` or `anyOf` variant that can be an
/// object, since a value such as `null` holds no fields.
fn requirements(root: &Value, node: &Value, seen: &BTreeSet<&str>) -> Vec<BTreeSet<String>> {
    let Value::Object(schema) = node else {
        return vec![BTreeSet::new()];
    };
    let names = (schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(Value::as_str)
    .map(str::to_owned);
    let mut ways = vec![names.collect::<BTreeSet<_>>()];
    let both = |ways: Vec<BTreeSet<String>>, others: Vec<BTreeSet<String>>| {
        let joined = ways
            .iter()
            .flat_map(|way| others.iter().map(move |other| way | other));
        joined.collect::<Vec<_>>()
    };
    if let Some((definition, seen)) = referenced(root, schema, seen) {
        ways = both(ways, requirements(root, definition, &seen));
    }
    for variant in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        ways = both(ways, requirements(root, variant, seen));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(Value::Array(variants)) = schema.get(key) {
            let objects = variants.iter().filter(|variant| match variant.get("type") {
                Some(Value::String(kind)) => kind == "object",
                Some(Value::Array(kinds)) => kinds.contains(&json!("object")),
                _ => true,
            });
            let any: Vec<_> = objects
                .flat_map(|variant| requirements(root, variant, seen))
                .collect();
            if !any.is_empty() {
                ways = both(ways, any);
            }
        }
    }
    ways
}

/// `ways` without a way that holds another, and empty when one requires
/// nothing.
fn least(mut ways: Vec<BTreeSet<String>>) -> Vec<BTreeSet<String>> {
    ways.sort();
    ways.dedup();
    let held =
        |way: &BTreeSet<String>| (ways.iter()).any(|other| other != way && other.is_subset(way));
    let least: Vec<BTreeSet<String>> = ways.iter().filter(|way| !held(way)).cloned().collect();
    if least.iter().any(BTreeSet::is_empty) {
        Vec::new()
    } else {
        least
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
    let in_types = relative.split('/').nth(2) == Some("types");
    let in_plugin = in_types
        && (relative.starts_with("plugins/") || relative.starts_with("crates/board-plugin-"));
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

/// What the source text of the plugin `name` spells as its codes: a
/// string literal that is `name` and two or more `::`-separated words, or a
/// format string that starts with a `{CONSTANT}` standing for `name`, as
/// `format!("{PLUGIN}::repo::not_synced")` does, into `codes`. A spelling
/// with a placeholder in a word, such as `"tasks::{call}::cross_repo"`, goes
/// into `dynamic` as it is spelled, for [`DYNAMIC`] to expand.
fn plugin_codes(
    name: &str,
    source: &str,
    codes: &mut BTreeSet<String>,
    dynamic: &mut BTreeSet<String>,
) {
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
        let placeholder = |word: &&str| word.starts_with('{') && word.ends_with('}');
        if words.len() < 3 || !words[0].is_empty() {
            continue;
        }
        if words[1..].iter().all(word) {
            codes.insert(format!("{name}{}", &rest[..end]));
        } else if words[1..].iter().all(|w| word(w) || placeholder(w)) {
            dynamic.insert(format!("{name}{}", &rest[..end]));
        }
    }
}

/// Each code a plugin spells with a placeholder, and the codes it builds.
/// [`plugins_codes`] checks that each word standing for a placeholder is a
/// literal in the plugin's source, and refuses a spelling not listed here.
const DYNAMIC: &[(&str, &[&str])] = &[
    (
        "boundary::crossing::{reason}",
        &[
            "boundary::crossing::not_found",
            "boundary::crossing::failed",
        ],
    ),
    (
        "plans::answer::{reason}",
        &[
            "plans::answer::already_answered",
            "plans::answer::no_question",
            "plans::answer::not_a_question_path",
            "plans::answer::not_open",
            "plans::answer::unknown_option",
            "plans::answer::words_needed",
        ],
    ),
    (
        "review::{area}::not_found",
        &["review::brief::not_found", "review::report::not_found"],
    ),
    (
        "tasks::{call}::cross_repo",
        &["tasks::add::cross_repo", "tasks::move::cross_repo"],
    ),
    (
        "tasks::{call}::parent_not_found",
        &[
            "tasks::add::parent_not_found",
            "tasks::move::parent_not_found",
        ],
    ),
];

/// The codes `spelled` builds, as [`DYNAMIC`] lists them, each checked
/// against `sources`; an error names a spelling it does not list.
fn expanded(spelled: &str, sources: &[&str]) -> Result<Vec<String>, String> {
    let Some((_, codes)) = DYNAMIC.iter().find(|(listed, _)| *listed == spelled) else {
        return Err(format!(
            "{spelled} is a code with a placeholder that DYNAMIC does not list; list the codes it builds"
        ));
    };
    for code in *codes {
        let pairs = spelled.split("::").zip(code.split("::"));
        for (placeholder, word) in pairs.filter(|(placeholder, _)| placeholder.starts_with('{')) {
            let quoted = format!("\"{word}\"");
            if !sources.iter().any(|source| source.contains(&quoted)) {
                return Err(format!(
                    "{code}: no source spells {quoted} for {placeholder}"
                ));
            }
        }
    }
    Ok(codes.iter().map(|code| (*code).to_owned()).collect())
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
    (
        "Writer",
        "crates/board-repos/src/writing.rs",
        &["repo::not_synced"],
    ),
];

/// Whether `source` builds `<plugin>::<suffix>`: in one format string, as
/// `{plugin}::<suffix>` or `{}::<suffix>`, or as
/// `<plugin>::<family>::{reason}` with the reason as its own literal.
fn builds(source: &str, suffix: &str) -> bool {
    let (family, reason) = suffix.split_once("::").unwrap();
    source.contains(&format!("\"{{plugin}}::{suffix}\""))
        || source.contains(&format!("\"{{}}::{suffix}\""))
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
    let (mut codes, mut unlisted) = (BTreeSet::new(), Vec::new());
    for (name, texts) in &by_plugin {
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        let mut dynamic = BTreeSet::new();
        for text in &texts {
            plugin_codes(name, text, &mut codes, &mut dynamic);
        }
        for spelled in dynamic {
            match expanded(&spelled, &texts) {
                Ok(built) => codes.extend(built),
                Err(error) => unlisted.push(error),
            }
        }
        shared_codes(name, &texts, &mut codes);
    }
    assert!(unlisted.is_empty(), "{}", unlisted.join("\n"));
    // A code a plugin's README promises must be one the scan found, so a
    // reason given to a listed placeholder does not slip past unlisted.
    for name in by_plugin.keys() {
        let readme = Path::new(ROOT).join(format!("plugins/{name}/README.md"));
        let Ok(text) = std::fs::read_to_string(&readme) else {
            continue;
        };
        let mut promised = BTreeSet::new();
        for quoted in text.split('`').skip(1).step_by(2) {
            plugin_codes(
                name,
                &format!("\"{quoted}\""),
                &mut promised,
                &mut BTreeSet::new(),
            );
        }
        let missed: Vec<&String> = promised.difference(&codes).collect();
        assert!(
            missed.is_empty(),
            "plugins/{name}/README.md promises {missed:?}, which no source spells"
        );
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
        let mut fields = BTreeMap::new();
        declared(
            &row["schema"],
            &row["schema"],
            "",
            &BTreeSet::new(),
            &mut fields,
        );
        // A report lists fields; an array's elements and a map's values
        // are not one.
        let field = |path: &String| !path.ends_with("[]") && !path.contains('*');
        let fields = fields.into_keys().filter(field);
        documents.insert(row["id"].as_str().unwrap().to_owned(), fields.collect());
    }
    assert_eq!(documents.len(), ids.len(), "schemas show left out an id");
    documents
}

/// Reports whose `data` is listed elsewhere or not at all: `query`'s is
/// the statement's rows, and `call`'s is a capability's output, which
/// [`capabilities`] lists. Their envelopes are frozen as their JSON Schemas
/// declare them.
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
    outputs.extend(rendered());
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

/// The reports of `serve`, `ui open`, `answer`, and of a `put` and a
/// `plugin enable` that hold every optional field, from the functions that
/// render them.
fn rendered() -> Vec<(Vec<&'static str>, Output)> {
    let mut rendered = Vec::new();
    let printer = |words: &[&str]| Printer::of(true, BUDGET, words);
    let served = Served {
        port: 1,
        requests: 1,
        idle: std::time::Duration::from_secs(1),
    };
    rendered.push((vec!["serve"], render::serve(printer(&["serve"]), &served)));
    let opened = Opened {
        binding: "/bindings/review".parse().unwrap(),
        port: 1,
        started: true,
    };
    let open = render::ui_open(printer(&["ui", "open"]), &opened);
    rendered.push((vec!["ui", "open"], open));
    let put = full_put();
    let answered = AnswerReport {
        id: "a".repeat(64),
        answer: Answer::Refuse,
        put: put.clone(),
    };
    rendered.push((vec!["put"], render::put(printer(&["put"]), &put, None)));
    let enabled = EnableReport {
        name: "tasks".parse().unwrap(),
        put: put.clone(),
        replaced: vec![".board/types/tasks/task/v2.schema.json".to_owned()],
        removed: vec![".board/types/tasks/old/v1.schema.json".to_owned()],
        kept: vec![Kept {
            schema: ".board/types/tasks/kept/v1.schema.json".to_owned(),
            record_type: "tasks/kept/v1".parse().unwrap(),
            records: 1,
        }],
    };
    let enable = render::plugin_enable(printer(&["plugin", "enable"]), &enabled);
    rendered.push((vec!["plugin", "enable"], enable));
    let answer = render::answer(printer(&["answer"]), &answered);
    rendered.push((vec!["answer"], answer));
    rendered
}

/// The fields a capability whose schemas are `input` and `output` declares.
fn fields(input: &Value, output: &Value) -> BTreeMap<String, Field> {
    let mut fields = BTreeMap::new();
    for (prefix, schema) in [("input", input), ("output", output)] {
        entered(schema, schema, prefix, &BTreeSet::new(), &mut fields);
    }
    fields
}

/// Each built-in plugin's capability with the fields its input and output
/// schemas declare, as `plugin find` lists them in a board.
fn capabilities() -> BTreeMap<String, BTreeMap<String, Field>> {
    let board = board();
    let context = Context {
        start: board.workdir().to_path_buf(),
        home: None,
        cache: None,
        plugins: None,
        boards: None,
        signer: None,
        allowed_signers: None,
        environment: board_plugin::Environment::default(),
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_plugins::BUILT_IN,
    };
    let arguments = [BINARY_NAME, "--json", "plugin", "find", "--all"].map(OsString::from);
    let found = run(arguments, &context, &mut &b""[..], &mut Ran::default());
    assert_eq!(found.code, 0, "{}", text(&found.stderr));
    let mut capabilities = BTreeMap::new();
    for row in json(&found)["data"].as_array().unwrap() {
        let name = format!(
            "{}.{}",
            row["plugin"].as_str().unwrap(),
            row["capability"].as_str().unwrap()
        );
        capabilities.insert(name, fields(&row["inputSchema"], &row["outputSchema"]));
    }
    capabilities
}

/// A put that holds every field a put report can print: a degraded write
/// reached through a mount, a removal, an absent path and a commit.
fn full_put() -> PutReport {
    let blob = Scenario::new().blob("{}");
    let board = |repository: &str| board_model::Board {
        repository: repository.into(),
        source: board_model::Source::WorkingTree,
    };
    let mount = Location {
        board: board("/a"),
        path: "vendor/lib.json".to_owned(),
        object: board_model::Object::Blob(blob),
        pointers: Vec::new(),
        follow: Some("main".to_owned()),
    };
    let written = Written {
        location: Location {
            board: board("/b"),
            path: "tasks/b.json".to_owned(),
            object: board_model::Object::Blob(blob),
            pointers: vec![mount],
            follow: None,
        },
        display: "/vendor/lib/tasks/b".parse().unwrap(),
        previous: Some(blob),
        blob,
        degraded: Some((
            Code::RefDangling,
            Detail {
                pointer: Some("/project".to_owned()),
                reason: "nothing is at /missing".to_owned(),
                failures: 2,
                at: Some("/vendor/lib".parse().unwrap()),
                mount: Some("/vendor/lib".parse().unwrap()),
                target: Some("/missing".to_owned()),
            },
        )),
    };
    PutReport {
        writes: vec![written],
        removed: vec![Removed {
            path: "/tasks/a".parse().unwrap(),
            blob,
        }],
        absent: vec!["/tasks/c".parse().unwrap()],
        commit: Some(blob),
    }
}

/// Every report, with the fields its JSON Schema declares and those its
/// samples hold.
fn reports() -> BTreeMap<String, BTreeSet<String>> {
    let mut reports = documents();
    for (schema, report) in samples() {
        let fields = (reports.get_mut(&schema))
            .unwrap_or_else(|| panic!("{schema} is printed but schemas list does not name it"));
        paths("data", &report["data"], fields);
        paths("summary", &report["summary"], fields);
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
        capabilities: capabilities(),
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
        "tasks::repo::not_synced",
        "tasks::move::parent_not_found",
        "boundary::crossing::failed",
        "board::path::not_found",
    ] {
        assert!(present.codes.contains(code), "{code}");
    }
    // Each report's `data` fields are listed, from its schema or its
    // samples, but for those whose `data` is not Board's.
    for (report, fields) in &present.reports {
        let holds_data =
            (fields.iter()).any(|field| field.starts_with("data.") || field.starts_with("data[]"));
        let expected = !OPEN_DATA.contains(&report.as_str()) && report != "board/failure/v1";
        assert_eq!(holds_data, expected, "{report}: {fields:?}");
    }
    let enable = &present.reports["board/plugin-enable-report/v2"];
    assert!(enable.contains("data.writes[].degraded.code"), "{enable:?}");
    assert!(enable.contains("data.kept[].records"), "{enable:?}");
    assert!(present.reports["board/check-report/v2"].contains("summary.degraded"));
    let check = &present.capabilities["controls.check"];
    assert!(
        check.contains_key("input.path") && check.contains_key("output.measured"),
        "{check:?}"
    );
    // Each capability field carries its type, read from the schemas Board
    // serves, so a capability another change adds is listed with no edit here.
    assert_eq!(check["input.path"].types, ["string".to_owned()].into());
    assert!(check["input"].requires.is_empty(), "{check:?}");
    assert_eq!(check["output.measured"].types, ["array".to_owned()].into());
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
fn a_value_added_to_an_enum_under_if_not_or_one_of_is_refused_and_one_added_beside_them_is_admitted()
 {
    let hasher = Scenario::new();
    let path = "crates/board-plugin-x/types/x/y/v1.schema.json";
    // As runs/run/v1 requires a note on a failed result.
    let schema = |under: &str, outcome: &[&str], beside: &[&str]| {
        let branch = json!({"properties": {"outcome": {"enum": outcome}}});
        let mut schema =
            json!({"properties": {"kind": {"enum": beside}}, "then": {"required": ["note"]}});
        schema[under] = if under == "oneOf" {
            json!([branch, {"required": ["x"]}])
        } else {
            branch
        };
        Released {
            types: [(
                path.to_owned(),
                frozen(&hasher, schema.to_string().as_bytes()),
            )]
            .into(),
            ..Released::default()
        }
    };
    for under in NARROWING {
        let released = schema(under, &["fail"], &["a"]);
        assert_eq!(
            violations(&released, &schema(under, &["fail"], &["a", "b"])),
            Vec::<String>::new(),
            "{under}"
        );
        assert_eq!(
            violations(&released, &schema(under, &["fail", "pass"], &["a"])),
            [format!("{path} changed; a changed type is a new version")],
            "{under}"
        );
    }
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
        let built = format!("runs::{call}::cross_repo");
    "#;
    let (mut codes, mut dynamic) = (BTreeSet::new(), BTreeSet::new());
    plugin_codes("runs", source, &mut codes, &mut dynamic);
    assert_eq!(
        codes.into_iter().collect::<Vec<_>>(),
        [
            "runs::a::b::c_2",
            "runs::repo::not_synced",
            "runs::suite::not_found"
        ]
    );
    assert_eq!(
        dynamic.into_iter().collect::<Vec<_>>(),
        ["runs::{call}::cross_repo"]
    );
}

#[test]
fn a_listed_placeholder_code_expands_when_its_words_are_spelled_and_an_unlisted_one_is_refused() {
    let spelled = "tasks::{call}::cross_repo";
    let both = r#"in_repo(.., "add"); in_repo(.., "move");"#;
    assert_eq!(
        expanded(spelled, &[both]),
        Ok(vec![
            "tasks::add::cross_repo".to_owned(),
            "tasks::move::cross_repo".to_owned()
        ])
    );
    assert_eq!(
        expanded(spelled, &[r#"in_repo(.., "add");"#]),
        Err(r#"tasks::move::cross_repo: no source spells "move" for {call}"#.to_owned())
    );
    assert_eq!(
        expanded("tasks::{call}::other", &[both]),
        Err("tasks::{call}::other is a code with a placeholder that DYNAMIC does not list; list the codes it builds".to_owned())
    );
}

#[test]
fn a_shared_code_is_built_by_its_source_in_one_string_or_from_its_family_and_reason() {
    let whole = r#"code: format!("{plugin}::board::refused"),"#;
    assert!(builds(whole, "board::refused"));
    assert!(builds(
        r#"format!("{}::repo::not_synced", writer.plugin)"#,
        "repo::not_synced"
    ));
    assert!(builds(
        r#"format!("{}::repo::not_synced", writer.plugin)"#,
        "repo::not_synced"
    ));
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
    let mut fields = BTreeMap::new();
    declared(&schema, &schema, "", &BTreeSet::new(), &mut fields);
    let fields = fields.into_keys().filter(|path| !path.ends_with("[]"));
    assert_eq!(
        fields.collect::<Vec<_>>(),
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
    assert!(!is_schema("plugins/effects/fixtures/x.schema.json"));
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
        capabilities: [(
            "x.y".to_owned(),
            [(format!("output.{field}"), Field::default())].into(),
        )]
        .into(),
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
            "x.y no longer holds output.data.gone",
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

/// A capability `x.y` whose input requires `required`, types `path` as
/// `kind` and admits `modes` as `mode`, and whose output types its rows'
/// `name` as `kind`, with `extra` set at each JSON pointer of the input.
fn capability(required: &[&str], kind: &str, modes: &[&str], extra: &[(&str, Value)]) -> Released {
    let mut input = json!({
        "type": "object",
        "required": required,
        "properties": {
            "path": {"type": kind},
            "mode": {"$ref": "#/$defs/Mode"},
            "filter": {"type": "object", "properties": {"tag": {"type": "string"}}},
        },
        "$defs": {"Mode": {"enum": modes}},
    });
    for (pointer, value) in extra {
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        input.pointer_mut(parent).unwrap()[key] = value.clone();
    }
    let output =
        json!({"properties": {"rows": {"items": {"properties": {"name": {"type": kind}}}}}});
    Released {
        capabilities: [("x.y".to_owned(), fields(&input, &output))].into(),
        ..Released::default()
    }
}

#[test]
fn a_capability_freezes_each_fields_types_what_each_object_requires_and_its_enum_values() {
    let fields = &capability(&["path"], "string", &["a"], &[]).capabilities["x.y"];
    let input = Field {
        types: ["object".to_owned()].into(),
        requires: vec![["path".to_owned()].into()],
        values: Vec::new(),
    };
    assert_eq!(fields["input"], input);
    assert_eq!(fields["input.path"].types, ["string".to_owned()].into());
    assert_eq!(fields["input.mode"].values, [json!("a")]);
    assert_eq!(
        fields["input.filter"].requires,
        Vec::<BTreeSet<String>>::new()
    );
    assert_eq!(
        fields["output.rows[].name"].types,
        ["string".to_owned()].into()
    );
    assert!(fields.contains_key("output.rows[]"), "{fields:?}");
}

/// A capability `x.y` whose input is `input` and whose output is `output`.
fn declaring(input: &Value, output: &Value) -> Released {
    Released {
        capabilities: [("x.y".to_owned(), fields(input, output))].into(),
        ..Released::default()
    }
}

#[test]
fn an_input_that_one_of_its_variants_requires_is_frozen_with_each_way_to_satisfy_it() {
    let input = |ways: Value| json!({"properties": {"subject": {}, "mount": {}, "limit": {}}, "oneOf": ways});
    let released = declaring(
        &input(json!([{"required": ["subject"]}, {"required": ["mount"]}])),
        &json!({}),
    );
    let either = [["mount".to_owned()].into(), ["subject".to_owned()].into()];
    assert_eq!(released.capabilities["x.y"]["input"].requires, either);
    let three =
        input(json!([{"required": ["subject"]}, {"required": ["mount"]}, {"required": ["limit"]}]));
    assert_eq!(
        violations(&released, &declaring(&three, &json!({}))),
        Vec::<String>::new(),
        "a new way to satisfy the input breaks no caller"
    );
    let any = json!({"properties": {"subject": {}, "mount": {}, "limit": {}}, "anyOf": [{"required": ["mount"]}, {"required": ["subject"]}]});
    assert_eq!(
        violations(&released, &declaring(&any, &json!({}))),
        Vec::<String>::new()
    );
    let one = input(json!([{"required": ["subject"]}]));
    assert_eq!(
        violations(&released, &declaring(&one, &json!({}))),
        ["x.y now requires input.subject; a newly required input is a new version"]
    );
    let more = input(json!([{"required": ["subject"]}, {"required": ["mount", "limit"]}]));
    assert_eq!(
        violations(&released, &declaring(&more, &json!({}))),
        ["x.y now requires input.limit; a newly required input is a new version"]
    );
}

#[test]
fn an_output_field_made_optional_is_refused_and_one_made_required_is_admitted() {
    let output =
        |required: &[&str]| json!({"properties": {"path": {}, "commit": {}}, "required": required});
    let released = declaring(&json!({}), &output(&["path"]));
    assert_eq!(
        violations(
            &released,
            &declaring(&json!({}), &output(&["path", "commit"]))
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        violations(&released, &declaring(&json!({}), &output(&[]))),
        ["x.y no longer always holds output.path; a reader relies on it"]
    );
}

#[test]
fn an_input_that_gains_an_enum_and_an_output_const_that_changes_are_refused_and_an_output_that_gains_an_enum_value_is_admitted()
 {
    let with = |input: Value, output: Value| {
        declaring(
            &json!({"properties": {"kind": input}}),
            &json!({"properties": {"to": output}}),
        )
    };
    let released = with(json!({"type": "string"}), json!({"const": "dropped"}));
    assert_eq!(
        violations(
            &released,
            &with(
                json!({"type": "string"}),
                json!({"enum": ["dropped", "done"]})
            )
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        violations(
            &released,
            &with(
                json!({"type": "string", "enum": ["a"]}),
                json!({"const": "done"})
            )
        ),
        [
            "x.y now limits input.kind to [\"a\"]; an enum on an input is a new version",
            "x.y no longer admits \"dropped\" at output.to; an enum value is never removed",
        ]
    );
}

#[test]
fn a_capability_input_made_optional_a_gained_enum_value_and_an_added_optional_input_are_admitted() {
    let released = capability(&["path"], "string", &["a", "b"], &[]);
    assert_eq!(violations(&released, &released), Vec::<String>::new());
    let relaxed = capability(
        &[],
        "string",
        &["a", "b", "c"],
        &[("/properties/limit", json!({"type": "integer"}))],
    );
    assert_eq!(violations(&released, &relaxed), Vec::<String>::new());
    let nested = json!({"type": "object", "required": ["id"], "properties": {"id": {}}});
    let added = capability(
        &["path"],
        "string",
        &["a", "b"],
        &[("/properties/new", nested)],
    );
    assert_eq!(
        violations(&released, &added),
        Vec::<String>::new(),
        "an input required only under a new optional input breaks no caller"
    );
}

#[test]
fn a_capability_input_newly_required_a_changed_type_and_a_lost_enum_value_are_refused() {
    let released = capability(&["path"], "string", &["a", "b"], &[]);
    let tightened = capability(&["path", "mode"], "integer", &["b"], &[]);
    assert_eq!(
        violations(&released, &tightened),
        [
            "x.y now requires input.mode; a newly required input is a new version",
            "x.y no longer admits \"a\" at input.mode; an enum value is never removed",
            "x.y changed the type of input.path from string to integer; a changed type is a new version",
            "x.y changed the type of output.rows[].name from string to integer; a changed type is a new version",
        ]
    );
    let limit = json!({"type": "integer"});
    let added = capability(
        &["path", "limit"],
        "string",
        &["a", "b"],
        &[
            ("/properties/limit", limit),
            ("/properties/filter/required", json!(["tag"])),
        ],
    );
    assert_eq!(
        violations(&released, &added),
        [
            "x.y now requires input.limit; a newly required input is a new version",
            "x.y now requires input.filter.tag; a newly required input is a new version",
        ]
    );
}

#[test]
fn an_enum_a_ref_reaches_from_under_if_not_or_one_of_stays_in_the_shape_and_one_reached_beside_them_is_emptied()
 {
    let hasher = Scenario::new();
    let path = "crates/board-plugin-x/types/x/y/v1.schema.json";
    let schema = |under: &str, outcome: &[&str], beside: &[&str]| {
        let branch = json!({"properties": {"outcome": {"$ref": "#/$defs/Outcome"}}});
        let mut schema = json!({
            "properties": {"kind": {"$ref": "#/$defs/Kind"}},
            "then": {"required": ["note"]},
            "$defs": {
                "Outcome": {"$ref": "#/$defs/Failed"},
                "Failed": {"enum": outcome},
                "Kind": {"enum": beside},
            },
        });
        schema[under] = if under == "oneOf" {
            json!([branch, {"required": ["x"]}])
        } else {
            branch
        };
        Released {
            types: [(
                path.to_owned(),
                frozen(&hasher, schema.to_string().as_bytes()),
            )]
            .into(),
            ..Released::default()
        }
    };
    for under in NARROWING {
        let released = schema(under, &["fail"], &["a"]);
        assert_eq!(
            violations(&released, &schema(under, &["fail"], &["a", "b"])),
            Vec::<String>::new(),
            "{under}"
        );
        assert_eq!(
            violations(&released, &schema(under, &["fail", "pass"], &["a"])),
            [format!("{path} changed; a changed type is a new version")],
            "{under}"
        );
    }
}

#[test]
fn a_nullable_output_object_keeps_what_it_requires_and_a_child_made_optional_is_refused() {
    // As schemars writes an `Option` of a struct.
    let output = |required: &[&str]| {
        json!({
            "properties": {"run": {"anyOf": [{"$ref": "#/$defs/Run"}, {"type": "null"}]}},
            "$defs": {"Run": {"type": "object", "properties": {"outcome": {}}, "required": required}},
        })
    };
    let released = declaring(&json!({}), &output(&["outcome"]));
    assert_eq!(
        violations(&released, &declaring(&json!({}), &output(&["outcome"]))),
        Vec::<String>::new()
    );
    assert_eq!(
        violations(&released, &declaring(&json!({}), &output(&[]))),
        ["x.y no longer always holds output.run.outcome; a reader relies on it"]
    );
}

#[test]
fn a_map_whose_value_type_changes_is_refused_and_one_whose_value_type_stays_is_admitted() {
    let input = |kind: &str| json!({"properties": {"parts": {"type": "object", "additionalProperties": {"type": kind}}}});
    let released = declaring(&input("string"), &json!({}));
    assert_eq!(
        violations(&released, &declaring(&input("string"), &json!({}))),
        Vec::<String>::new()
    );
    assert_eq!(
        violations(&released, &declaring(&input("integer"), &json!({}))),
        [
            "x.y changed the type of input.parts.* from string to integer; a changed type is a new version"
        ]
    );
}
