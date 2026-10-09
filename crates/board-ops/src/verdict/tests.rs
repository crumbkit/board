//! Judging a blob by its bytes alone, as a pure function.

use board_model::{Code, Content, Role};
use proptest::prelude::*;

use super::judge;
use crate::tests::verdicts::{case, records, schemas};
use crate::tests::{MANIFEST, TASK_SCHEMA};

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
                    Content::Record { record_type, .. } if record_type.is_board())
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
