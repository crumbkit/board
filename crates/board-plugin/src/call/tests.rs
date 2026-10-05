use std::path::Path;

use serde_json::json;

use crate::installed::Installed;
use crate::process::Limits;

use super::*;

#[test]
fn a_target_is_a_plugin_and_capability_joined_by_a_dot() {
    assert_eq!(
        "tasks.due".parse::<Target>().unwrap(),
        Target::Named {
            plugin: "tasks".parse().unwrap(),
            capability: "due".parse().unwrap(),
        }
    );
    for refused in ["tasks", "tasks.", ".due", "tasks.due.x", "Tasks.due"] {
        assert!(refused.parse::<Target>().is_err(), "{refused}");
    }
}

#[test]
fn a_target_without_a_dot_is_refused_naming_the_form_it_needs() {
    assert_eq!(
        "tasks".parse::<Target>().unwrap_err().to_string(),
        r#""tasks" is not <plugin>.<capability>, such as tasks.due"#
    );
    assert_eq!(
        "tasks.".parse::<Target>().unwrap_err().to_string(),
        r#""tasks." is not <plugin>.<capability>, such as tasks.due"#
    );
    assert_eq!(
        "Tasks.due".parse::<Target>().unwrap_err().to_string(),
        r#""Tasks" is not a name: use lowercase ASCII letters, digits and hyphens"#
    );
    assert!("tasks.due".parse::<Target>().is_ok());
}

#[test]
fn a_depth_below_the_limit_is_admitted_and_one_at_it_refused() {
    let at = |depth: &str| Environment {
        depth: Some(OsString::from(depth)),
        ..Environment::default()
    };
    assert_eq!(Environment::default().depth().unwrap(), 0);
    assert_eq!(at("7").depth().unwrap(), 7);
    assert!(matches!(
        at("8").depth(),
        Err(CallError::TooDeep { depth: 8 })
    ));
    assert!(matches!(at("8 ").depth(), Err(CallError::BadDepth { .. })));
}

/// An install of `name` that offers `capabilities`, each taking `input`.
fn plugin(name: &str, capabilities: &[&str], input: &Value) -> Installed {
    let manifest = json!({
        "$type": "board/plugin-manifest/v1",
        "name": name,
        "version": "0.1.0",
        "description": "A test plugin.",
        "run": ["./missing"],
        "capabilities": capabilities.iter().map(|capability| json!({
            "name": capability,
            "description": "A test capability.",
            "inputSchema": input,
            "outputSchema": {"type": "object"},
        })).collect::<Vec<_>>(),
    });
    Installed {
        name: name.to_owned(),
        directory: PathBuf::from(name),
        manifest: Ok(Manifest::parse(name, manifest.to_string().as_bytes()).unwrap()),
    }
}

fn plugins(offers: &[(&str, &[&str])]) -> Vec<Installed> {
    offers
        .iter()
        .map(|(name, capabilities)| plugin(name, capabilities, &json!({"type": "object"})))
        .collect()
}

fn names<T: FromStr<Err = NameError>>(names: &[&str]) -> Vec<T> {
    names.iter().map(|name| name.parse().unwrap()).collect()
}

/// The code, message and fields of the refusal `result` holds.
fn refusal(result: Result<Chosen, CallError>) -> (Option<Code>, String, Value) {
    let error = result.unwrap_err();
    (
        error.code(),
        error.to_string(),
        Value::Object(error.fields()),
    )
}

#[test]
fn a_named_capability_is_chosen_among_the_plugins_others() {
    let chosen = choose(
        plugins(&[("tasks", &["due", "open"])]),
        &names(&["tasks"]),
        &"tasks.open".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(chosen.capability().name.as_str(), "open");
    assert_eq!(chosen.directory, PathBuf::from("tasks"));
}

#[test]
fn a_capability_the_named_plugin_lacks_is_refused_listing_what_it_offers() {
    let installed = || plugins(&[("tasks", &["due", "open"]), ("bare", &[])]);
    assert_eq!(
        refusal(choose(
            installed(),
            &names(&["tasks"]),
            &"tasks.closed".parse().unwrap()
        )),
        (
            Some(Code::PluginNotInstalled),
            "plugin tasks has no capability closed; it offers due, open".to_owned(),
            json!({"plugin": "tasks", "capability": "closed", "offered": ["due", "open"]}),
        )
    );
    assert_eq!(
        refusal(choose(
            installed(),
            &names(&["bare"]),
            &"bare.due".parse().unwrap()
        ))
        .1,
        "plugin bare has no capability due; it offers none"
    );
    assert!(
        choose(
            installed(),
            &names(&["tasks"]),
            &"tasks.due".parse().unwrap()
        )
        .is_ok()
    );
}

#[test]
fn a_named_plugin_that_is_missing_or_not_enabled_is_refused_by_name() {
    assert_eq!(
        refusal(choose(
            plugins(&[("tasks", &["due"])]),
            &names(&["tasks"]),
            &"notes.due".parse().unwrap()
        )),
        (
            Some(Code::PluginNotInstalled),
            "plugin notes is not installed".to_owned(),
            json!({"plugin": "notes"}),
        )
    );
    assert_eq!(
        refusal(choose(
            plugins(&[("tasks", &["due"])]),
            &[],
            &"tasks.due".parse().unwrap()
        )),
        (
            Some(Code::PluginNotEnabled),
            "plugin tasks is installed but this board does not enable it".to_owned(),
            json!({"plugin": "tasks"}),
        )
    );
}

#[test]
fn a_capability_alone_is_chosen_from_the_one_enabled_plugin_offering_it() {
    let chosen = choose(
        plugins(&[("notes", &["due"]), ("tasks", &["due", "open"])]),
        &names(&["tasks"]),
        &Target::Capability("open".parse().unwrap()),
    )
    .unwrap();
    assert_eq!(chosen.manifest.name.as_str(), "tasks");
    assert_eq!(chosen.capability().name.as_str(), "open");
}

#[test]
fn a_capability_offered_by_two_enabled_plugins_is_refused_naming_both() {
    let installed = || plugins(&[("notes", &["due"]), ("tasks", &["due"])]);
    assert_eq!(
        refusal(choose(
            installed(),
            &names(&["notes", "tasks"]),
            &Target::Capability("due".parse().unwrap())
        )),
        (
            Some(Code::PluginAmbiguous),
            "capability due is offered by more than one enabled plugin: notes, tasks".to_owned(),
            json!({"capability": "due", "plugins": ["notes", "tasks"]}),
        )
    );
    assert!(
        choose(
            installed(),
            &names(&["tasks"]),
            &Target::Capability("due".parse().unwrap())
        )
        .is_ok()
    );
}

#[test]
fn a_capability_nobody_offers_is_refused_with_the_nearest_name() {
    let installed = || plugins(&[("tasks", &["due", "open"])]);
    let call = |capability: &str| {
        refusal(choose(
            installed(),
            &names(&["tasks"]),
            &Target::Capability(capability.parse().unwrap()),
        ))
    };
    assert_eq!(
        call("dues"),
        (
            Some(Code::PluginNotInstalled),
            "no installed plugin offers capability dues; the nearest is due".to_owned(),
            json!({"capability": "dues", "nearest": "due"}),
        )
    );
    assert_eq!(
        call("closed"),
        (
            Some(Code::PluginNotInstalled),
            "no installed plugin offers capability closed".to_owned(),
            json!({"capability": "closed"}),
        )
    );
    assert!(
        choose(
            installed(),
            &names(&["tasks"]),
            &Target::Capability("due".parse().unwrap())
        )
        .is_ok()
    );
}

/// The capability nearest `wanted` among `offered`.
fn nearest_of(wanted: &str, offered: &[&str]) -> Option<String> {
    let manifests: Vec<_> = plugins(&[("tasks", offered)])
        .into_iter()
        .map(|installed| (installed.manifest.unwrap(), installed.directory))
        .collect();
    nearest(&wanted.parse().unwrap(), &manifests).map(|name| name.as_str().to_owned())
}

#[test]
fn the_nearest_name_is_within_two_edits_and_fewer_than_the_names_letters() {
    assert_eq!(nearest_of("dueby", &["due"]).as_deref(), Some("due"));
    assert_eq!(nearest_of("duebye", &["due"]), None);
    assert_eq!(nearest_of("ab", &["a"]).as_deref(), Some("a"));
    assert_eq!(nearest_of("ab", &["xy"]), None);
    assert_eq!(nearest_of("abc", &["xyc"]).as_deref(), Some("xyc"));
}

#[test]
fn the_nearest_name_prefers_fewer_edits_then_the_first_in_order() {
    assert_eq!(
        nearest_of("tasks", &["tusky", "tasky"]).as_deref(),
        Some("tasky")
    );
    assert_eq!(
        nearest_of("tasks", &["tasky", "masks"]).as_deref(),
        Some("masks")
    );
}

/// The refusal of `input` by a capability whose input schema is `schema`.
fn input_refusal(schema: &Value, input: &str) -> (Option<Code>, String, Value) {
    let chosen = choose(
        vec![plugin("tasks", &["due"], schema)],
        &names(&["tasks"]),
        &"tasks.due".parse().unwrap(),
    )
    .unwrap();
    let environment = Environment::default();
    let context = Context {
        root: Path::new("."),
        database: Path::new("tasks.sqlite"),
        environment: &environment,
        limits: Limits::default(),
    };
    let error = invoke(&chosen, input.as_bytes(), &context).unwrap_err();
    (
        error.code(),
        error.to_string(),
        Value::Object(error.fields()),
    )
}

#[test]
fn input_that_fails_the_schema_is_refused_naming_where() {
    let schema = json!({
        "type": "object",
        "properties": {
            "before": {"type": "string"},
            "task": {"type": "string", "format": "board-ref"},
        },
    });
    assert_eq!(
        input_refusal(&schema, "[]"),
        (
            Some(Code::PluginBadInput),
            r#"the input fails the capability's input schema at its root: [] is not of type "object""#
                .to_owned(),
            json!({"capability": "due", "pointer": ""}),
        )
    );
    assert_eq!(
        input_refusal(&schema, r#"{"before": 5}"#),
        (
            Some(Code::PluginBadInput),
            r#"the input fails the capability's input schema at "/before": 5 is not of type "string""#
                .to_owned(),
            json!({"capability": "due", "pointer": "/before"}),
        )
    );
    assert_eq!(
        input_refusal(&schema, r#"{"task": "/a//b"}"#),
        (
            Some(Code::PluginBadInput),
            r#"the input fails the capability's input schema at "/task": the reference is not a logical path"#
                .to_owned(),
            json!({"capability": "due", "pointer": "/task"}),
        )
    );
    let (code, message, fields) = input_refusal(&schema, "{");
    assert_eq!(code, Some(Code::PluginBadInput));
    assert!(
        message.starts_with("the input is not one strict JSON document: "),
        "{message}"
    );
    assert_eq!(fields, json!({"capability": "due"}));
    let admitted = input_refusal(&schema, r#"{"before": "2026-10-05", "task": "/a/b"}"#);
    assert_eq!(admitted.0, Some(Code::PluginFailed), "{}", admitted.1);
}
