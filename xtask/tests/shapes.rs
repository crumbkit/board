//! Every record schema committed in the repository keeps the shape rules
//! that a reading of the schema can check
//! (docs/architecture/records/shapes.md). Each string field uses a shared
//! shape (rule 14), and the four names rules 6 and 7 reserve use theirs.
//! The other rules on naming, enums and `$defs` need judgement and are left
//! to review.

#[path = "shapes/exempt.rs"]
mod exempt;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use exempt::{Kind, UNSHARED};

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
        let schema = edited(&kept, pointer, value);
        let rules: Vec<&str> = broken(&schema).into_iter().map(|(rule, _)| rule).collect();
        assert_eq!(rules, [rule]);
    }
}

/// `schema` with the value at `pointer` set to `value`, or removed when
/// `value` is null.
#[cfg(test)]
fn edited(schema: &Value, pointer: &str, value: Value) -> Value {
    let mut schema = schema.clone();
    let (parent, key) = pointer.rsplit_once('/').unwrap();
    let parent = schema.pointer_mut(parent).unwrap().as_object_mut().unwrap();
    if value.is_null() {
        parent.remove(key);
    } else {
        parent.insert(key.to_owned(), value);
    }
    schema
}

/// `schema` without `description` and `title`, which say what a field means
/// and leave its shape alone.
#[cfg(test)]
fn bare(schema: &Value) -> Value {
    let mut bare = schema.clone();
    if let Some(keywords) = bare.as_object_mut() {
        keywords.remove("description");
        keywords.remove("title");
    }
    bare
}

/// The shape a field with a name that rules 6 and 7 reserve must have, and
/// whether the field is a list whose items have it. `None` when the shape is
/// not in `registry` yet.
#[cfg(test)]
fn reserved(name: &str, registry: &Map<String, Value>) -> Option<(bool, Option<Value>)> {
    let mount = json!({"type": "string", "format": "board-ref", "x-board-type": "board/mount/v1"});
    match name {
        "mount" => Some((false, Some(mount))),
        "mounts" => Some((true, Some(mount))),
        "remote" => Some((false, registry.get("Remote").cloned())),
        "urls" => Some((true, registry.get("Url").cloned())),
        _ => None,
    }
}

/// The keywords whose subschemas the walk reads or knowingly leaves alone.
/// Any other keyword that holds a schema fails the lint, so a keyword the
/// walk does not read cannot hide a string. `if`, `not` and
/// `dependentRequired` only constrain which fields are present.
#[cfg(test)]
const READ: [&str; 20] = [
    "$ref",
    "properties",
    "patternProperties",
    "additionalProperties",
    "items",
    "prefixItems",
    "anyOf",
    "oneOf",
    "allOf",
    "then",
    "else",
    "$defs",
    "if",
    "not",
    "required",
    "dependentRequired",
    "enum",
    "const",
    "default",
    "examples",
];

/// The keywords that, in a schema with no `type`, say it shapes something
/// other than a bare string: a reference to another schema, a union, a
/// value list, an object or array, or which fields an object holds, as an
/// `anyOf` of `required` lists or a `then` does. A schema with no `type` and
/// none of them admits any string.
#[cfg(test)]
const SHAPED: [&str; 14] = [
    "$ref",
    "anyOf",
    "oneOf",
    "allOf",
    "enum",
    "const",
    "properties",
    "patternProperties",
    "additionalProperties",
    "items",
    "prefixItems",
    "required",
    "dependentRequired",
    "not",
];

/// Each string field in `schema` that is not an inline enum or const, a
/// reference, prose or an entry of `registry`, each field with a reserved
/// name that lacks its shape, and each schema the walk cannot read: the
/// field's pointer in a record, with what is wrong.
#[cfg(test)]
fn unshared(schema: &Value, registry: &Map<String, Value>) -> Vec<(String, String)> {
    let mut walk = Walk {
        root: schema,
        registry,
        through: Vec::new(),
        reserved: BTreeSet::new(),
        found: Vec::new(),
    };
    walk.node(schema, "");
    walk.found
}

/// The state of one walk over a record schema.
#[cfg(test)]
struct Walk<'a> {
    /// The schema whose `$defs` a local `$ref` names.
    root: &'a Value,
    registry: &'a Map<String, Value>,
    /// The `$defs` the walk is inside, so a recursive type ends.
    through: Vec<String>,
    /// The pointers whose fields the reserved-name check already judged.
    reserved: BTreeSet<String>,
    found: Vec<(String, String)>,
}

#[cfg(test)]
impl Walk<'_> {
    /// Checks each string schema in `node`, which describes the value at
    /// `at` in a record.
    fn node(&mut self, node: &Value, at: &str) {
        let any = Map::new();
        let keywords = match node {
            Value::Bool(true) => &any,
            Value::Bool(false) => return,
            Value::Object(keywords) => keywords,
            _ => return self.refuse(at, format!("{node} is no schema")),
        };
        for (keyword, value) in keywords {
            let holds_schema = value.is_object()
                || (value.as_array()).is_some_and(|values| values.iter().any(Value::is_object));
            if holds_schema && !READ.contains(&keyword.as_str()) {
                self.refuse(at, format!("the shapes lint does not read `{keyword}`"));
            }
        }
        if let Some(target) = keywords.get("$ref") {
            self.follow(target, at);
        }
        for union in ["anyOf", "oneOf", "allOf"] {
            for branch in (keywords.get(union).and_then(Value::as_array))
                .into_iter()
                .flatten()
            {
                self.node(branch, at);
            }
        }
        for branch in ["then", "else"].iter().filter_map(|key| keywords.get(*key)) {
            self.node(branch, at);
        }
        let string = match keywords.get("type") {
            None => !SHAPED.iter().any(|keyword| keywords.contains_key(*keyword)),
            Some(Value::Array(types)) => types.contains(&"string".into()),
            Some(kind) => kind == "string",
        };
        let shape = bare(node);
        if string && !self.reserved.contains(at) && !shared(&shape, self.registry) {
            self.refuse(at, format!("{shape} is no shared shape"));
        }
        for (name, property) in (keywords.get("properties").and_then(Value::as_object))
            .into_iter()
            .flatten()
        {
            let place = format!("{at}/{name}");
            if let Some((list, wanted)) = reserved(name, self.registry) {
                let (value, pointer) = if list {
                    (&property["items"], format!("{place}/-"))
                } else {
                    (property, place.clone())
                };
                if wanted.as_ref() != Some(&bare(value)) {
                    let wanted = wanted
                        .map_or("a shape shapes.json does not hold yet".into(), |wanted| {
                            wanted.to_string()
                        });
                    self.refuse(&pointer, format!("`{name}` is reserved for {wanted}"));
                }
                self.reserved.insert(pointer);
            }
            self.node(property, &place);
        }
        let patterned = (keywords.get("patternProperties").and_then(Value::as_object))
            .into_iter()
            .flat_map(|map| map.values());
        for values in keywords
            .get("additionalProperties")
            .into_iter()
            .chain(patterned)
        {
            self.node(values, &format!("{at}/{{key}}"));
        }
        if let Some(items) = keywords.get("items") {
            self.node(items, &format!("{at}/-"));
        }
        for (index, item) in (keywords.get("prefixItems").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .enumerate()
        {
            self.node(item, &format!("{at}/{index}"));
        }
    }

    /// Walks the `$defs` entry that `target` names, once on each path.
    fn follow(&mut self, target: &Value, at: &str) {
        let root = self.root;
        let named = (target
            .as_str()
            .and_then(|target| target.strip_prefix("#/$defs/")))
        .and_then(|name| Some((name, root.get("$defs")?.get(name)?)));
        let Some((name, definition)) = named else {
            return self.refuse(
                at,
                format!(
                    "the shapes lint reads only a `$ref` to an entry of `#/$defs/`, not {target}"
                ),
            );
        };
        if self.through.iter().any(|inside| inside == name) {
            return;
        }
        self.through.push(name.to_owned());
        self.node(definition, at);
        self.through.pop();
    }

    fn refuse(&mut self, at: &str, why: String) {
        self.found.push((at.to_owned(), why));
    }
}

/// Whether the string schema `shape`, without `description` and `title`, is
/// one rule 14 admits.
#[cfg(test)]
fn shared(shape: &Value, registry: &Map<String, Value>) -> bool {
    let prose = shape["x-board-prose"] == true
        && shape.as_object().is_some_and(|keywords| {
            (keywords.keys()).all(|key| {
                ["type", "x-board-prose", "minLength", "maxLength"].contains(&key.as_str())
            })
        });
    shape.get("enum").is_some()
        || shape.get("const").is_some()
        || shape["format"] == "board-ref"
        || prose
        || registry.values().any(|entry| entry == shape)
}

/// The shared shapes, as the committed `crates/board-plugin/shapes.json`
/// holds them.
#[cfg(test)]
fn registry() -> Map<String, Value> {
    let text = fs::read(root().join("crates/board-plugin/shapes.json")).unwrap();
    serde_json::from_slice(&text).unwrap()
}

#[test]
fn every_string_field_of_a_committed_record_schema_uses_a_shared_shape_or_is_listed_as_exempt() {
    let registry = registry();
    assert!(registry.contains_key("Prose"), "{registry:?}");
    let mut used = BTreeSet::new();
    let mut failures = Vec::new();
    for (name, schema) in &schemas() {
        for (pointer, why) in unshared(schema, &registry) {
            match UNSHARED
                .iter()
                .position(|entry| (entry.0, entry.1) == (name, &pointer))
            {
                Some(index) => {
                    used.insert(index);
                }
                None => failures.push(format!("{name} {pointer}: {why}")),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "each string field is an inline enum or const, a reference, Prose, or a shared shape from \
         crates/board-plugin/shapes.json (shapes rule 14); a meaning no shape owns gets a new shape \
         in board_plugin::shape:\n{failures:#?}"
    );
    let unused: Vec<String> = (UNSHARED.iter().enumerate())
        .filter(|(index, _)| !used.contains(index))
        .map(|(_, (name, pointer, kind, _))| match kind {
            Kind::UntilAdopted => format!("{name} {pointer} now uses its shape"),
            Kind::Local => format!("{name} {pointer} no longer needs its local shape"),
        })
        .collect();
    assert!(unused.is_empty(), "remove these entries: {unused:#?}");
    assert!(UNSHARED.iter().all(|entry| !entry.3.is_empty()));
}

#[test]
fn each_string_shape_the_lint_refuses_has_an_admitted_twin() {
    let kept = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["$type"],
        "properties": {
            "$type": {"type": "string", "const": "a/b/v1", "description": "Type."},
            "mood": {"type": "string", "enum": ["calm"], "description": "Mood."},
            "note": {"type": "string", "x-board-prose": true, "minLength": 1, "description": "Note."},
            "due": {"type": "string", "format": "date", "title": "Due", "description": "Due."},
            "maybe": {"type": ["string", "null"], "enum": ["a", null], "description": "Maybe."},
            "count": {"type": "integer", "description": "Count."},
            "after": {"type": "string", "format": "board-ref", "description": "After."},
            "either": {"description": "Either.", "anyOf": [{"type": "integer"}, {"type": "string", "format": "date"}]},
            "one": {"description": "One.", "oneOf": [{"type": "integer"}, {"type": "string", "format": "date"}]},
            "all": {"description": "All.", "allOf": [{"type": "string", "format": "date"}]},
            "labels": {
                "description": "Labels.",
                "type": "object",
                "additionalProperties": {"type": "string", "x-board-prose": true}
            },
            "named": {
                "description": "Named.",
                "type": "object",
                "additionalProperties": false,
                "patternProperties": {"^[a-z]+$": {"type": "string", "enum": ["a"]}}
            },
            "tags": {"description": "Tags.", "type": "array", "items": {"type": "string", "enum": ["a"]}},
            "pair": {"description": "Pair.", "type": "array", "prefixItems": [{"type": "integer"}, {"type": "string", "enum": ["a"]}]},
            "inner": {"$ref": "#/$defs/Inner", "description": "Inner."},
            "mount": {"type": "string", "format": "board-ref", "x-board-type": "board/mount/v1", "description": "Mount."},
            "mounts": {
                "description": "Mounts.",
                "type": "array",
                "items": {"type": "string", "format": "board-ref", "x-board-type": "board/mount/v1"}
            },
            "remote": {"type": "string", "pattern": "^r$", "description": "Remote."},
            "urls": {"description": "Pages.", "type": "array", "items": {"type": "string", "format": "uri"}}
        },
        "if": {"required": ["mood"]},
        "then": {"properties": {"note": {"x-board-prose": true}}},
        "else": {"properties": {"note": {"x-board-prose": true}}},
        "$defs": {
            "Inner": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "name": {"type": "string", "const": "n", "description": "Name."},
                    "child": {"$ref": "#/$defs/Inner", "description": "Child."}
                }
            }
        }
    });
    let mut registry = registry();
    registry.insert("Remote".into(), json!({"type": "string", "pattern": "^r$"}));
    registry.insert("Url".into(), json!({"type": "string", "format": "uri"}));
    assert_eq!(unshared(&kept, &registry), []);
    let changes: [(&str, Value, &str); 24] = [
        ("/properties/mood/enum", Value::Null, "/mood"),
        ("/properties/note/x-board-prose", Value::Null, "/note"),
        ("/properties/note/pattern", json!("^a"), "/note"),
        ("/properties/due/format", json!("date-time"), "/due"),
        ("/properties/maybe/enum", Value::Null, "/maybe"),
        ("/properties/count/type", Value::Null, "/count"),
        ("/properties/count", json!(true), "/count"),
        ("/properties/after/format", Value::Null, "/after"),
        (
            "/properties/either/anyOf/1/format",
            json!("time"),
            "/either",
        ),
        ("/properties/one/oneOf/1/format", json!("time"), "/one"),
        ("/properties/all/allOf/0/format", json!("time"), "/all"),
        (
            "/properties/labels/additionalProperties/x-board-prose",
            Value::Null,
            "/labels/{key}",
        ),
        (
            "/properties/named/patternProperties/^[a-z]+$/enum",
            Value::Null,
            "/named/{key}",
        ),
        ("/properties/tags/items/enum", Value::Null, "/tags/-"),
        (
            "/properties/pair/prefixItems/1/enum",
            Value::Null,
            "/pair/1",
        ),
        (
            "/properties/tags/contains",
            json!({"type": "string", "enum": ["a"]}),
            "/tags",
        ),
        ("/then/properties/note/x-board-prose", Value::Null, "/note"),
        ("/else/properties/note/pattern", json!("^a"), "/note"),
        (
            "/$defs/Inner/properties/name/const",
            Value::Null,
            "/inner/name",
        ),
        (
            "/properties/inner/$ref",
            json!("#/definitions/Inner"),
            "/inner",
        ),
        (
            "/properties/mount/x-board-type",
            json!("plans/question/v1"),
            "/mount",
        ),
        (
            "/properties/mounts/items/x-board-type",
            Value::Null,
            "/mounts/-",
        ),
        ("/properties/remote/pattern", json!("^s$"), "/remote"),
        ("/properties/urls/items/format", json!("email"), "/urls/-"),
    ];
    for (pointer, value, refused) in changes {
        let found: Vec<String> = (unshared(&edited(&kept, pointer, value), &registry).into_iter())
            .map(|(pointer, _)| pointer)
            .collect();
        assert_eq!(found, [refused], "{pointer}");
    }
    let without: Vec<String> = (unshared(&kept, &self::registry()).into_iter())
        .map(|(pointer, _)| pointer)
        .collect();
    assert_eq!(without, ["/remote", "/urls/-"]);
}
