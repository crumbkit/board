//! The schema of a flow's check. A check is an object of several kinds,
//! named by its `kind`, so each kind's parameters go in a `$defs` entry of
//! their own, gathered through `allOf`. An entry requires its kind's
//! required parameters when `kind` names the kind and refuses its
//! parameters otherwise, and `unevaluatedProperties` refuses a field that
//! neither the check nor any kind names. The Rust struct stays flat, so
//! serde and the gates read every parameter from one place.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{Map, Value, json};

use super::{Check, Kind};

/// One kind that takes parameters: the kind, its `$defs` entry, its
/// parameters, and those it requires.
struct Parameters {
    kind: Kind,
    entry: &'static str,
    named: &'static [&'static str],
    required: &'static [&'static str],
}

/// The parameters of each kind that takes any. `paths`, `tasks`,
/// `considered` and `flow-valid` take none.
const PARAMETERS: [Parameters; 10] = [
    Parameters {
        kind: Kind::Sections,
        entry: "SectionsCheck",
        named: &["sections"],
        required: &["sections"],
    },
    Parameters {
        kind: Kind::Field,
        entry: "FieldCheck",
        named: &["field", "types", "min"],
        required: &["field"],
    },
    Parameters {
        kind: Kind::Records,
        entry: "RecordsCheck",
        named: &["type", "min", "distinct", "where"],
        required: &["type"],
    },
    Parameters {
        kind: Kind::Run,
        entry: "RunCheck",
        named: &["scope"],
        required: &[],
    },
    Parameters {
        kind: Kind::Understood,
        entry: "UnderstoodCheck",
        named: &["subject", "rechecks"],
        required: &["subject"],
    },
    Parameters {
        kind: Kind::Mockups,
        entry: "MockupsCheck",
        named: &["part"],
        required: &[],
    },
    Parameters {
        kind: Kind::Obligations,
        entry: "ObligationsCheck",
        named: &["escape_section"],
        required: &[],
    },
    Parameters {
        kind: Kind::ControlledPaths,
        entry: "ControlledPathsCheck",
        named: &["controls"],
        required: &["controls"],
    },
    Parameters {
        kind: Kind::PlannedControlled,
        entry: "PlannedControlledCheck",
        named: &["controls"],
        required: &["controls"],
    },
    Parameters {
        kind: Kind::Exists,
        entry: "ExistsCheck",
        named: &["path"],
        required: &["path"],
    },
];

/// The schema of a [`Check`], with each kind's parameters in its own
/// `$defs` entry. `Phase::checks` takes it with `#[schemars(with)]`.
#[derive(Debug)]
pub(crate) enum Checked {}

impl JsonSchema for Checked {
    fn schema_name() -> Cow<'static, str> {
        "Check".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = Check::json_schema(generator);
        schema.remove("additionalProperties");
        let properties = (schema.get_mut("properties").and_then(Value::as_object_mut))
            .expect("a check's schema names its properties");
        let mut moved = Map::new();
        for parameter in PARAMETERS.iter().flat_map(|kind| kind.named) {
            if let Some(property) = properties.remove(*parameter) {
                moved.insert((*parameter).to_owned(), property);
            }
        }
        let kinds: Vec<Value> = (PARAMETERS.iter())
            .map(|kind| entry(generator, kind, &moved))
            .collect();
        schema.insert("allOf".to_owned(), Value::from(kinds));
        schema.insert("unevaluatedProperties".to_owned(), Value::Bool(false));
        schema
    }
}

/// `kind`'s parameters, taken from `moved`, as its `$defs` entry, and a
/// reference to it.
fn entry(generator: &mut SchemaGenerator, kind: &Parameters, moved: &Map<String, Value>) -> Value {
    let name = kind.kind.as_str();
    let properties: Map<String, Value> = (kind.named.iter())
        .map(|parameter| {
            let property = moved
                .get(*parameter)
                .expect("each kind's parameter is a field of Check");
            ((*parameter).to_owned(), property.clone())
        })
        .collect();
    let refused: Vec<Value> = (kind.named.iter())
        .map(|parameter| refused_unless_other_kind(kind.kind, parameter))
        .collect();
    let mut schema = json!({
        "description": format!("The parameters of a check of kind `{name}`."),
        "properties": properties,
        "if": {"properties": {"kind": {"const": name}}, "required": ["kind"]},
        "else": {"not": {"anyOf": refused}},
    });
    if !kind.required.is_empty() {
        schema["then"] = json!({"required": kind.required});
    }
    generator
        .definitions_mut()
        .insert(kind.entry.to_owned(), schema);
    json!({"$ref": format!("#/$defs/{}", kind.entry)})
}

/// What a check other than `kind` matches when it holds `parameter`: the
/// parameter, unless another kind that also takes it is the check's kind.
fn refused_unless_other_kind(kind: Kind, parameter: &str) -> Value {
    let others: Vec<&str> = (PARAMETERS.iter())
        .filter(|other| other.kind != kind && other.named.contains(&parameter))
        .map(|other| other.kind.as_str())
        .collect();
    if others.is_empty() {
        return json!({"required": [parameter]});
    }
    json!({
        "required": [parameter],
        "not": {"properties": {"kind": {"enum": others}}, "required": ["kind"]},
    })
}
