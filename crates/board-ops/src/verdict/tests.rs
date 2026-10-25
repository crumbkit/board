//! Judging a blob by its bytes alone, as a pure function.

use board_model::{Code, Content, Role, parse_json};
use board_tree::Refused;
use board_validate::Schema;
use proptest::prelude::*;

use std::cell::Cell;

use super::{Admitted, SchemaFound, against, judge, record};
use crate::tests::verdicts::{Case, TASK_SCHEMA_PATH, case, records, schemas};
use crate::tests::{MANIFEST, TASK_SCHEMA, task};

/// The code `judge` gives `bytes` read in `role`, or `None` when it admits them.
fn code(role: Role, bytes: &str) -> Option<Code> {
    judge(&role.classify(Some(bytes.as_bytes())))
        .err()
        .map(|fault| fault.code())
}

#[test]
fn a_configuration_with_an_unknown_field_is_refused_and_its_twin_is_admitted() {
    let config = |fields: &str| format!(r#"{{"$type": "board/config/v1"{fields}}}"#);
    assert_eq!(
        code(Role::Json, &config(r#", "colour": "red""#)),
        Some(Code::ConfigInvalid)
    );
    assert_eq!(
        code(Role::Json, &config(r#", "commit_on_put": true"#)),
        None
    );
}

#[test]
fn a_manifest_that_runs_nothing_is_refused_with_its_own_code_and_its_twin_is_admitted() {
    let empty = MANIFEST.replace(r#"["./run"]"#, "[]");
    assert_eq!(code(Role::Json, &empty), Some(Code::PluginBadManifest));
    assert_eq!(code(Role::Json, MANIFEST), None);
}

#[test]
fn a_schema_that_does_not_compile_is_refused_and_one_that_compiles_is_returned_compiled() {
    let broken = Role::Schema.classify(Some(br#"{"type": 5}"#));
    assert_eq!(
        judge(&broken).err().map(|fault| fault.code()),
        Some(Code::SchemaInvalid)
    );
    let task = Role::Schema.classify(Some(TASK_SCHEMA.as_bytes()));
    assert!(matches!(judge(&task), Ok(Some(_))));
}

#[test]
fn a_schema_in_the_reserved_namespace_is_refused_and_the_same_bytes_elsewhere_are_admitted() {
    assert_eq!(
        code(Role::ReservedSchema, TASK_SCHEMA),
        Some(Code::SchemaInvalid)
    );
    assert_eq!(code(Role::Schema, TASK_SCHEMA), None);
}

#[test]
fn a_record_of_a_users_type_waits_for_its_schema_and_bytes_that_do_not_parse_are_refused() {
    assert!(matches!(
        judge(&Role::Json.classify(Some(br#"{"$type": "lr/task/v1"}"#))),
        Ok(None)
    ));
    assert_eq!(code(Role::Json, "{"), Some(Code::JsonParse));
}

/// Whether `judge` keeps the code classifying `bytes` in `role` gives, and
/// otherwise refuses only a schema, the configuration or a manifest, and
/// compiles exactly the schemas.
fn keeps_the_rule(role: Role, bytes: &str) -> bool {
    let classified = role.classify(Some(bytes.as_bytes()));
    let judged = judge(&classified);
    if let Some(code) = classified.code {
        return judged.err().map(|fault| fault.code()) == Some(code);
    }
    let schema = matches!(classified.content, Content::Schema { .. });
    match judged {
        Err(_) => {
            schema
                || matches!(&classified.content,
                    Content::Record { record_type, .. } if record_type.is_reserved())
        }
        Ok(compiled) => compiled.is_some() == schema,
    }
}

const ROLES: [Role; 3] = [Role::Json, Role::Schema, Role::ReservedSchema];

#[test]
fn judge_keeps_the_rule_for_every_listed_record_and_schema_in_every_role() {
    let listed = records().into_iter().chain(schemas().into_iter().flatten());
    for bytes in listed {
        for role in ROLES {
            assert!(keeps_the_rule(role, &bytes), "{bytes:?} as {role:?}");
        }
    }
}

proptest! {
    #[test]
    fn judge_keeps_the_rule_for_generated_records_in_every_role(
        records in case().prop_map(|case| case.records),
        role in prop::sample::select(ROLES.to_vec()),
    ) {
        for bytes in records {
            prop_assert!(keeps_the_rule(role, &bytes), "{:?} as {:?}", bytes, role);
        }
    }
}

fn task_schema() -> Schema {
    Schema::compile(parse_json(TASK_SCHEMA.as_bytes()).unwrap()).unwrap()
}

/// What `record` makes of `bytes` when the task schema's path holds
/// `found`, or the task schema when `found` is `None`.
fn recorded(bytes: &str, found: Option<SchemaFound>) -> Result<Admitted, Code> {
    let schema = task_schema();
    let find = |_: &_| found.map_or(Ok(&schema), Err);
    record(Role::Json.classify(Some(bytes.as_bytes())), find).map_err(|fault| fault.code())
}

#[test]
fn a_record_takes_the_code_of_what_is_at_its_schema_path_and_its_twin_under_a_usable_schema_is_admitted()
 {
    for (found, code) in [
        (SchemaFound::Missing, Code::RecordUnknownType),
        (SchemaFound::Unreadable, Code::RepoUnreadable),
        (SchemaFound::Unusable, Code::SchemaInvalid),
    ] {
        assert_eq!(recorded(&task(""), Some(found)).err(), Some(code));
    }
    assert!(recorded(&task(""), None).is_ok());
}

#[test]
fn a_record_without_a_usable_schema_is_refused_naming_its_type_and_schema_file() {
    for (found, message) in [
        (
            SchemaFound::Missing,
            format!("no schema for lr/task/v1 at {TASK_SCHEMA_PATH}"),
        ),
        (
            SchemaFound::Unreadable,
            format!("its schema at {TASK_SCHEMA_PATH} could not be read"),
        ),
        (
            SchemaFound::Unusable,
            format!("its schema at {TASK_SCHEMA_PATH} does not compile"),
        ),
        (
            SchemaFound::Symlink(Refused {
                code: Code::SymlinkDangling,
                target: "gone.json".to_owned(),
            }),
            format!(
                "its schema at {TASK_SCHEMA_PATH} is a symlink to \"gone.json\", which names nothing"
            ),
        ),
    ] {
        let classified = Role::Json.classify(Some(task("").as_bytes()));
        let fault = record(classified, |_| Err(found.clone())).unwrap_err();
        assert_eq!(fault.to_string(), message);
        let fields = fault.fields();
        assert_eq!(fields.schema.as_deref(), Some(TASK_SCHEMA_PATH));
        assert_eq!(fields.record_type.unwrap().as_str(), "lr/task/v1");
        let pointer = (found == SchemaFound::Unusable).then_some("/$type");
        assert_eq!(fields.pointer.as_deref(), pointer);
        let target = matches!(found, SchemaFound::Symlink(_)).then_some("gone.json");
        assert_eq!(fields.target.as_deref(), target);
    }
}

#[test]
fn a_valid_record_gives_the_references_its_schema_finds_and_its_invalid_twin_is_refused() {
    let admitted = recorded(&task(r#", "project": "/projects/p""#), None).unwrap();
    let targets: Vec<String> = admitted
        .references
        .iter()
        .map(|reference| reference.path.to_string())
        .collect();
    assert_eq!(targets, ["/projects/p"]);
    assert!(!admitted.pointer);
    assert_eq!(admitted.value["project"], "/projects/p");
    let invalid = recorded(r#"{"$type": "lr/task/v1", "title": 5}"#, None);
    assert_eq!(invalid.err(), Some(Code::RecordInvalid));
    let error = against(&task_schema(), &serde_json::json!({"title": 5})).unwrap_err();
    assert_eq!(error.fields().pointer.as_deref(), Some("/title"));
}

#[test]
fn a_pointer_and_boards_own_types_are_admitted_without_a_schema_and_their_broken_twins_are_refused()
{
    let unasked = |_: &_| -> Result<&Schema, SchemaFound> { panic!("no schema is needed") };
    let mount = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    let config = r#"{"$type": "board/config/v1"}"#;
    for (bytes, pointer) in [(mount, true), (config, false), (MANIFEST, false)] {
        let admitted = record(Role::Json.classify(Some(bytes.as_bytes())), unasked).unwrap();
        assert_eq!(admitted.pointer, pointer, "{bytes}");
        assert!(admitted.references.is_empty());
        assert_eq!(admitted.value, parse_json(bytes.as_bytes()).unwrap());
    }
    for (bytes, code) in [
        (r#"{"$type": "board/mount/v1"}"#, Code::MountInvalid),
        (
            r#"{"$type": "board/config/v1", "colour": "red"}"#,
            Code::ConfigInvalid,
        ),
        (
            &MANIFEST.replace(r#"["./run"]"#, "[]"),
            Code::PluginBadManifest,
        ),
    ] {
        let refused = record(Role::Json.classify(Some(bytes.as_bytes())), unasked);
        assert_eq!(
            refused.err().map(|fault| fault.code()),
            Some(code),
            "{bytes}"
        );
    }
}

#[test]
fn an_effects_record_is_admitted_without_a_schema_and_its_twin_with_a_bad_pattern_is_refused() {
    let unasked = |_: &_| -> Result<&Schema, SchemaFound> { panic!("no schema is needed") };
    let preference = |path: &str| {
        format!(
            r#"{{"$type": "board/preference/v1", "op": "put", "path": "{path}", "authority": "ask"}}"#
        )
    };
    let admitted = preference("/tasks/*");
    assert!(record(Role::Json.classify(Some(admitted.as_bytes())), unasked).is_ok());
    let refused = preference("/tasks/[ab]");
    let refused = record(Role::Json.classify(Some(refused.as_bytes())), unasked);
    assert_eq!(
        refused.err().map(|fault| fault.code()),
        Some(Code::RecordInvalid)
    );
    let seen =
        r#"{"$type": "board/seen/v1", "effect": "/effects/pending/0a", "answer": "approve"}"#;
    assert_eq!(code(Role::Json, seen), None);
    assert_eq!(
        code(Role::Json, &seen.replace("approve", "yes")),
        Some(Code::RecordInvalid)
    );
    let effect = r#"{"$type": "board/effect/v1", "board": "/b", "plugin": "tasks", "op": "put", "path": "/x", "authority": "ask"}"#;
    assert_eq!(code(Role::Json, effect), None);
    assert_eq!(
        code(Role::Json, &effect.replace("ask", "maybe")),
        Some(Code::RecordInvalid)
    );
}

#[test]
fn plain_json_is_not_a_record_and_its_typed_twin_is_admitted() {
    let plain = record(Role::Json.classify(Some(b"{}")), |_| {
        Err(SchemaFound::Missing)
    });
    assert_eq!(
        plain.err().map(|fault| fault.code()),
        Some(Code::PutInvalid)
    );
    assert!(recorded(&task(""), None).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_record_judge_refuses_has_judges_code_and_one_whose_schema_is_not_usable_has_that_states_code(
        Case { records, .. } in case(),
    ) {
        let states = [
            (SchemaFound::Missing, Code::RecordUnknownType),
            (SchemaFound::Unreadable, Code::RepoUnreadable),
            (SchemaFound::Unusable, Code::SchemaInvalid),
            (
                SchemaFound::Symlink(Refused {
                    code: Code::SymlinkLoop,
                    target: "a.json".to_owned(),
                }),
                Code::SymlinkLoop,
            ),
        ];
        for bytes in records {
            let judged = code(Role::Json, &bytes);
            let mut unasked = Vec::new();
            for (found, its) in states.clone() {
                let asked = Cell::new(false);
                let classified = Role::Json.classify(Some(bytes.as_bytes()));
                let refused = record(classified, |_| {
                    asked.set(true);
                    Err(found.clone())
                })
                .err()
                .map(|fault| fault.code());
                if judged.is_some() {
                    prop_assert_eq!(refused, judged, "{:?}", bytes);
                } else if asked.get() {
                    prop_assert_eq!(refused, Some(its), "{:?} with {:?}", bytes, found);
                } else {
                    unasked.push(refused);
                }
            }
            // A record that needs no schema has one verdict whatever is there.
            unasked.dedup();
            prop_assert!(unasked.len() <= 1, "{:?}: {:?}", bytes, unasked);
        }
    }
}
