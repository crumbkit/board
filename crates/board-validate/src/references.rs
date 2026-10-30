//! Finding the references a record makes (docs/architecture.md, Typed
//! references). The schema is searched through `properties`,
//! `patternProperties`, `additionalProperties`, `items`, `prefixItems`,
//! `allOf` and local `$ref`. A schema that marks a reference under any
//! other keyword that applies subschemas, or behind a `$dynamicRef`, is
//! refused.

use std::collections::HashSet;

use board_model::{LogicalPath, RecordType};
use regex::Regex;
use serde_json::{Map, Value};

use crate::{Accepts, REFERENCE_FORMAT, SchemaError};

/// The most local `$ref`s followed without descending the record, so that a
/// `$ref` cycle in a schema ends.
const MAX_REFS: usize = 32;

/// The most subschemas one record location may expand to through the
/// keywords in [`IN_PLACE`]. Branches multiply, so without this a small
/// schema from a mounted board could make one record cost billions of
/// visits, in this search or in the validator.
pub(crate) const MAX_EXPANSION: usize = 256;

/// The keywords that apply subschemas at the instance location they sit at,
/// besides `$ref` and `$dynamicRef`.
const IN_PLACE: [&str; 8] = [
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
];

/// Refuses a schema in which some subschema expands, through [`IN_PLACE`]
/// keywords and local references, to more than [`MAX_EXPANSION`]
/// subschemas at one record location, or makes a reference this search
/// cannot follow. Every subschema in the document is tried, since any of
/// them can be where a record location starts.
pub(crate) fn check_expansion(schema: &Value) -> Result<(), SchemaError> {
    let mut pending = vec![schema];
    while let Some(node) = pending.pop() {
        match node {
            Value::Object(object) => {
                let mut count = 0;
                if !within_expansion(schema, node, 0, &mut count) {
                    return Err(SchemaError::TooComplex);
                }
                pending.extend(object.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    Ok(())
}

/// Whether `node` expands to at most [`MAX_EXPANSION`] subschemas, adding
/// each to `count`. A reference that is not `#` or a JSON pointer to a
/// subschema of `root` fails, as it could hide any expansion.
pub(crate) fn within_expansion(root: &Value, node: &Value, refs: usize, count: &mut usize) -> bool {
    *count += 1;
    if *count > MAX_EXPANSION {
        return false;
    }
    let Value::Object(keywords) = node else {
        return true;
    };
    let branches = IN_PLACE
        .iter()
        .flat_map(|keyword| match keywords.get(*keyword) {
            Some(Value::Array(branches)) => branches.iter().collect(),
            Some(Value::Object(by_name)) if *keyword == "dependentSchemas" => {
                by_name.values().collect()
            }
            Some(schema) => vec![schema],
            None => Vec::new(),
        });
    for branch in branches {
        if !within_expansion(root, branch, refs, count) {
            return false;
        }
    }
    for keyword in ["$ref", "$dynamicRef"] {
        let Some(target) = keywords.get(keyword).and_then(Value::as_str) else {
            continue;
        };
        if refs >= MAX_REFS {
            continue;
        }
        let followed = local(root, target)
            .is_some_and(|target| within_expansion(root, target, refs + 1, count));
        if !followed {
            return false;
        }
    }
    true
}

/// The keywords whose subschemas are searched for references, besides
/// `$ref`, which keeps the search where it was.
const SEARCHED: [&str; 6] = [
    "properties",
    "patternProperties",
    "additionalProperties",
    "items",
    "prefixItems",
    "allOf",
];

/// Keywords whose value is a map of subschemas, a list of them, or one.
const MAPS: [&str; 4] = [
    "$defs",
    "properties",
    "patternProperties",
    "dependentSchemas",
];
const LISTS: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
const SINGLE: [&str; 10] = [
    "items",
    "additionalProperties",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// Refuses a schema that marks a reference under a keyword outside
/// [`SEARCHED`], or behind a `$dynamicRef`, directly or through local
/// `$ref`s, since the record's reference would validate but never be found.
pub(crate) fn check_hidden(schema: &Value) -> Result<(), SchemaError> {
    Hidden {
        root: schema,
        followed: HashSet::new(),
    }
    .walk(schema, None)
}

struct Hidden<'a> {
    root: &'a Value,
    /// Each reference already followed, with the keyword it sat under.
    followed: HashSet<(&'a str, Option<&'static str>)>,
}

impl<'a> Hidden<'a> {
    /// Walks the subschemas of `node`, which sits under the unsearched
    /// keyword `under`, if any.
    fn walk(&mut self, node: &'a Value, under: Option<&'static str>) -> Result<(), SchemaError> {
        let Value::Object(keywords) = node else {
            return Ok(());
        };
        if let Some(keyword) = under
            && keywords.get("format").and_then(Value::as_str) == Some(REFERENCE_FORMAT)
        {
            return Err(SchemaError::HiddenReference { keyword });
        }
        for (keyword, inner) in [("$ref", under), ("$dynamicRef", Some("$dynamicRef"))] {
            if let Some(target) = keywords.get(keyword).and_then(Value::as_str)
                && let Some(found) = local(self.root, target)
                && self.followed.insert((target, inner))
            {
                self.walk(found, inner)?;
            }
        }
        for (keyword, value) in keywords {
            let Some((name, schemas)) = applied(keyword, value) else {
                continue;
            };
            let inner = match name {
                "$defs" => None,
                searched if SEARCHED.contains(&searched) => under,
                unsearched => Some(unsearched),
            };
            for schema in schemas {
                self.walk(schema, inner)?;
            }
        }
        Ok(())
    }
}

/// The subschemas `value` holds as the value of `keyword`, with the
/// keyword's name; `None` for a keyword that holds none.
fn applied<'a>(keyword: &str, value: &'a Value) -> Option<(&'static str, Vec<&'a Value>)> {
    let name = *(MAPS.iter().chain(&LISTS).chain(&SINGLE)).find(|name| **name == keyword)?;
    let schemas = match value {
        Value::Object(by_name) if MAPS.contains(&name) => by_name.values().collect(),
        Value::Array(schemas) if LISTS.contains(&name) => schemas.iter().collect(),
        schema if SINGLE.contains(&name) => vec![schema],
        _ => return None,
    };
    Some((name, schemas))
}

/// A reference a record makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// Where the reference is in the record, as a JSON pointer.
    pub pointer: String,
    pub path: LogicalPath,
    /// The record types the target may have, from `x-board-type`.
    pub accepts: Accepts,
}

/// Refuses a schema whose `x-board-type` anywhere is not a record type or a
/// list of them.
pub(crate) fn check_types(schema: &Value) -> Result<(), SchemaError> {
    match schema {
        Value::Object(object) => {
            if let Some(value) = object.get("x-board-type") {
                accepts(value).ok_or_else(|| SchemaError::BoardType {
                    value: value.to_string(),
                })?;
            }
            object.values().try_for_each(check_types)
        }
        Value::Array(items) => items.iter().try_for_each(check_types),
        _ => Ok(()),
    }
}

fn accepts(value: &Value) -> Option<Vec<RecordType>> {
    match value {
        Value::String(text) => Some(vec![text.parse().ok()?]),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str()?.parse().ok())
            .collect(),
        _ => None,
    }
}

/// The references in `record` under `schema`, in document order: object
/// keys sorted, array items by index. A reference that several subschemas
/// mark is reported once.
pub(crate) fn collect(schema: &Value, record: &Value) -> Vec<Reference> {
    let mut found = Vec::new();
    Search {
        root: schema,
        found: &mut found,
    }
    .visit(schema, record, &mut Vec::new(), 0, &mut 0);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.dedup_by(|a, b| a.0 == b.0);
    found.into_iter().map(|(_, reference)| reference).collect()
}

/// One step into a JSON value, ordered as a document reads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Index(usize),
    Key(String),
}

fn pointer(steps: &[Step]) -> String {
    steps
        .iter()
        .map(|step| match step {
            Step::Index(index) => format!("/{index}"),
            Step::Key(key) => format!("/{}", key.replace('~', "~0").replace('/', "~1")),
        })
        .collect()
}

struct Search<'a> {
    root: &'a Value,
    found: &'a mut Vec<(Vec<Step>, Reference)>,
}

impl Search<'_> {
    /// Visits `schema` at one instance location. `expanded` counts the
    /// subschemas visited there through `allOf` and `$ref`, which compile
    /// already bounds; the count here only guards that bound.
    fn visit(
        &mut self,
        schema: &Value,
        instance: &Value,
        steps: &mut Vec<Step>,
        refs: usize,
        expanded: &mut usize,
    ) {
        *expanded += 1;
        if *expanded > MAX_EXPANSION {
            return;
        }
        let Value::Object(keywords) = schema else {
            return;
        };
        if keywords.get("format").and_then(Value::as_str) == Some(REFERENCE_FORMAT)
            && let Some(path) = instance.as_str().and_then(|text| text.parse().ok())
        {
            let reference = Reference {
                pointer: pointer(steps),
                path,
                accepts: keywords.get("x-board-type").and_then(accepts),
            };
            self.found.push((steps.clone(), reference));
        }
        if let Some(target) = keywords.get("$ref").and_then(Value::as_str)
            && refs < MAX_REFS
            && let Some(target) = local(self.root, target)
        {
            self.visit(target, instance, steps, refs + 1, expanded);
        }
        for branch in keywords
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.visit(branch, instance, steps, refs, expanded);
        }
        match instance {
            Value::Object(members) => self.members(keywords, members, steps),
            Value::Array(items) => self.items(keywords, items, steps),
            _ => {}
        }
    }

    fn members(
        &mut self,
        keywords: &Map<String, Value>,
        members: &Map<String, Value>,
        steps: &mut Vec<Step>,
    ) {
        let properties = keywords.get("properties").and_then(Value::as_object);
        let patterns: Vec<(Regex, &Value)> = keywords
            .get("patternProperties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(pattern, schema)| Some((ecma(pattern)?, schema)))
            .collect();
        let additional = keywords.get("additionalProperties");
        for (key, value) in members {
            steps.push(Step::Key(key.clone()));
            let mut matched = false;
            if let Some(schema) = properties.and_then(|properties| properties.get(key)) {
                matched = true;
                self.visit(schema, value, steps, 0, &mut 0);
            }
            for (pattern, schema) in &patterns {
                if pattern.is_match(key) {
                    matched = true;
                    self.visit(schema, value, steps, 0, &mut 0);
                }
            }
            if !matched && let Some(schema) = additional {
                self.visit(schema, value, steps, 0, &mut 0);
            }
            steps.pop();
        }
    }

    fn items(&mut self, keywords: &Map<String, Value>, items: &[Value], steps: &mut Vec<Step>) {
        let prefix = keywords
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for (index, value) in items.iter().enumerate() {
            let schema = prefix.get(index).or_else(|| keywords.get("items"));
            if let Some(schema) = schema {
                steps.push(Step::Index(index));
                self.visit(schema, value, steps, 0, &mut 0);
                steps.pop();
            }
        }
    }
}

/// `pattern` compiled as the validator compiles it: read with ECMAScript
/// rules, so `\d` is an ASCII digit, then built for the linear-time engine.
fn ecma(pattern: &str) -> Option<Regex> {
    Regex::new(&jsonschema_regex::to_rust_regex(pattern).ok()?).ok()
}

/// The subschema a local `$ref` names in `root`: `#`, or a JSON pointer
/// under it.
fn local<'a>(root: &'a Value, target: &str) -> Option<&'a Value> {
    let pointer = target.strip_prefix('#')?;
    if pointer.is_empty() {
        Some(root)
    } else {
        root.pointer(pointer)
    }
}
