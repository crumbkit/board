use std::path::Path;

use board_model::PluginCode;
use board_model::RefusalCode::{self, Board};
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
        r#""tasks" is not <plugin>.<capability>, such as tasks.available"#
    );
    assert_eq!(
        "tasks.".parse::<Target>().unwrap_err().to_string(),
        r#""tasks." is not <plugin>.<capability>, such as tasks.available"#
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

#[test]
fn a_plugin_inherits_board_timings_when_it_is_set_and_not_when_it_is_not() {
    let off = Environment {
        timings: Some(OsString::from("0")),
        ..Environment::default()
    };
    assert!(environment(&off, &[], 1, &tasks()).contains(&("BOARD_TIMINGS", OsString::from("0"))));
    let empty = Environment::default();
    let unset = environment(&empty, &[], 1, &tasks());
    assert!(!unset.iter().any(|(key, _)| *key == "BOARD_TIMINGS"));
}

#[test]
fn a_plugin_inherits_tz_when_it_is_set_and_not_when_it_is_not() {
    let far = Environment {
        tz: Some(OsString::from("Pacific/Kiritimati")),
        ..Environment::default()
    };
    assert!(
        environment(&far, &[], 1, &tasks()).contains(&("TZ", OsString::from("Pacific/Kiritimati")))
    );
    let empty = Environment::default();
    let unset = environment(&empty, &[], 1, &tasks());
    assert!(!unset.iter().any(|(key, _)| *key == "TZ"));
}

#[test]
fn a_plugin_inherits_the_caller_s_directory_and_day_start_when_set_and_not_when_not() {
    let set = Environment {
        caller: Some(OsString::from("/work/home")),
        day_start: Some(OsString::from("07:00")),
        ..Environment::default()
    };
    let passed = environment(&set, &[], 1, &tasks());
    assert!(passed.contains(&("BOARD_CALLER_DIR", OsString::from("/work/home"))));
    assert!(passed.contains(&("BOARD_DAY_START", OsString::from("07:00"))));
    let empty = Environment::default();
    let unset = environment(&empty, &[], 1, &tasks());
    let keys = ["BOARD_CALLER_DIR", "BOARD_DAY_START"];
    assert!(!unset.iter().any(|(key, _)| keys.contains(key)));
}

#[test]
fn a_plugin_sees_its_name_in_board_plugin_and_inherits_xdg_config_home_only_when_set() {
    let set = Environment {
        config: Some(OsString::from("/config")),
        ..Environment::default()
    };
    let seen = environment(&set, &[], 1, &tasks());
    assert!(seen.contains(&("BOARD_PLUGIN", OsString::from("tasks"))));
    assert!(seen.contains(&("XDG_CONFIG_HOME", OsString::from("/config"))));
    let empty = Environment::default();
    let unset = environment(&empty, &[], 1, &tasks());
    assert!(!unset.iter().any(|(key, _)| *key == "XDG_CONFIG_HOME"));
}

#[test]
fn a_plugin_called_from_inside_another_plugin_keeps_the_outer_plugins_name() {
    let inside = Environment {
        plugin: Some(OsString::from("review")),
        ..Environment::default()
    };
    let seen = environment(&inside, &[], 2, &tasks());
    let named: Vec<_> = (seen.iter())
        .filter(|(key, _)| *key == "BOARD_PLUGIN")
        .collect();
    assert_eq!(named, [&("BOARD_PLUGIN", OsString::from("review"))]);
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
        schemas: Vec::new(),
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
fn refusal(result: Result<Chosen, CallError>) -> (RefusalCode, String, Value) {
    let refusal = result.unwrap_err().refusal();
    (refusal.code, refusal.message, Value::Object(refusal.fields))
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
            Board(Code::PluginNotInstalled),
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
            Board(Code::PluginNotInstalled),
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
            Board(Code::PluginNotEnabled),
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
            Board(Code::PluginAmbiguous),
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
            Board(Code::PluginNotInstalled),
            "no installed plugin offers capability dues; the nearest is due".to_owned(),
            json!({"capability": "dues", "nearest": "due"}),
        )
    );
    assert_eq!(
        call("closed"),
        (
            Board(Code::PluginNotInstalled),
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
        .map(|installed| installed.manifest.unwrap())
        .collect();
    nearest(&wanted.parse().unwrap(), manifests.iter()).map(|name| name.as_str().to_owned())
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
fn input_refusal(schema: &Value, input: &str) -> (RefusalCode, String, Value) {
    let chosen = choose(
        vec![plugin("tasks", &["due"], schema)],
        &names(&["tasks"]),
        &"tasks.due".parse().unwrap(),
    )
    .unwrap();
    let environment = Environment::default();
    let context = Context {
        root: Path::new("."),
        environment: &environment,
        limits: Limits::default(),
        spawned: &RefCell::default(),
    };
    let refusal = invoke(&chosen, input.as_bytes(), &context)
        .unwrap_err()
        .refusal();
    (refusal.code, refusal.message, Value::Object(refusal.fields))
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
            Board(Code::PluginBadInput),
            r#"the input to capability due fails its schema at its root: [] is not of type "object""#
                .to_owned(),
            json!({"plugin": "tasks", "capability": "due", "pointer": ""}),
        )
    );
    assert_eq!(
        input_refusal(&schema, r#"{"before": 5}"#),
        (
            Board(Code::PluginBadInput),
            r#"the input to capability due fails its schema at "/before": 5 is not of type "string""#
                .to_owned(),
            json!({"plugin": "tasks", "capability": "due", "pointer": "/before"}),
        )
    );
    assert_eq!(
        input_refusal(&schema, r#"{"task": "/a//b"}"#),
        (
            Board(Code::PluginBadInput),
            r#"the input to capability due holds a reference at "/task": "/a//b" is not a logical path"#
                .to_owned(),
            json!({"plugin": "tasks", "capability": "due", "pointer": "/task"}),
        )
    );
    let (code, message, fields) = input_refusal(&schema, "{");
    assert_eq!(code, Board(Code::PluginBadInput));
    assert!(
        message.starts_with("the input is not one strict JSON document: "),
        "{message}"
    );
    assert_eq!(fields, json!({"plugin": "tasks", "capability": "due"}));
    let admitted = input_refusal(&schema, r#"{"before": "2026-10-05", "task": "/a/b"}"#);
    assert_eq!(admitted.0, Board(Code::PluginNotStarted), "{}", admitted.1);
}

/// Asserts that Board refuses `error` with `code`, naming `fields`, with the
/// hint `code` gives for the plugin and capability it names, and `stderr`.
fn assert_refused(error: &CallError, code: Code, fields: &Value, stderr: &[u8]) {
    let named = |key: &str| fields.get(key).and_then(Value::as_str);
    let hinted = Fields {
        plugin: named("plugin").map(|name| name.parse().unwrap()),
        capability: named("capability").map(|name| name.parse().unwrap()),
        ..Fields::default()
    };
    assert_eq!(
        error.refusal(),
        Refusal {
            code: RefusalCode::Board(code),
            message: error.to_string(),
            hint: Some(code.hint(&hinted)),
            fields: fields.as_object().unwrap().clone(),
            stderr: stderr.to_vec(),
            lines: Vec::new(),
        },
        "{error}"
    );
}

fn tasks() -> PluginName {
    "tasks".parse().unwrap()
}

fn due() -> CapabilityName {
    "due".parse().unwrap()
}

#[test]
fn a_call_board_refuses_before_choosing_a_capability_has_its_code_and_the_names_at_fault() {
    let unreadable = DiscoveryError::Unreadable {
        directory: PathBuf::from("plugins"),
        source: std::io::Error::other("gone"),
    };
    let discovery = CallError::Discovery { source: unreadable };
    assert_refused(&discovery, Code::PluginNotInstalled, &json!({}), b"");
    let depth = CallError::BadDepth { text: "x".into() };
    assert_refused(&depth, Code::PluginTooDeep, &json!({}), b"");
    let deep = CallError::TooDeep { depth: 8 };
    assert_refused(&deep, Code::PluginTooDeep, &json!({}), b"");
    let missing = CallError::NotInstalled { plugin: tasks() };
    assert_refused(
        &missing,
        Code::PluginNotInstalled,
        &json!({"plugin": "tasks"}),
        b"",
    );
    let lacking = CallError::NoCapability {
        plugin: tasks(),
        capability: due(),
        offered: names(&["open"]),
    };
    let named = json!({"plugin": "tasks", "capability": "due", "offered": ["open"]});
    assert_refused(&lacking, Code::PluginNotInstalled, &named, b"");
    let near = CallError::NotOffered {
        capability: due(),
        nearest: Some("dew".parse().unwrap()),
    };
    let nearest = json!({"capability": "due", "nearest": "dew"});
    assert_refused(&near, Code::PluginNotInstalled, &nearest, b"");
    let far = CallError::NotOffered {
        capability: due(),
        nearest: None,
    };
    assert_refused(
        &far,
        Code::PluginNotInstalled,
        &json!({"capability": "due"}),
        b"",
    );
    let manifest = CallError::BadManifest {
        plugin: tasks(),
        source: ManifestError::Missing,
    };
    assert_refused(
        &manifest,
        Code::PluginBadManifest,
        &json!({"plugin": "tasks"}),
        b"",
    );
    let disabled = CallError::NotEnabled { plugin: tasks() };
    assert_refused(
        &disabled,
        Code::PluginNotEnabled,
        &json!({"plugin": "tasks"}),
        b"",
    );
}

#[test]
fn a_call_board_refuses_before_the_plugin_runs_has_its_code_and_the_values_at_fault() {
    let torn = CallError::InputJson {
        plugin: tasks(),
        capability: due(),
        source: parse_json(b"{").unwrap_err(),
    };
    assert_refused(
        &torn,
        Code::PluginBadInput,
        &json!({"plugin": "tasks", "capability": "due"}),
        b"",
    );
    let malformed = CallError::Input {
        plugin: tasks(),
        capability: due(),
        source: RecordError::MalformedReference {
            pointer: "/task".to_owned(),
            value: json!("/a//b"),
        },
    };
    let at = json!({"plugin": "tasks", "capability": "due", "pointer": "/task"});
    assert_refused(&malformed, Code::PluginBadInput, &at, b"");
    let start = CallError::Start {
        plugin: tasks(),
        source: std::io::Error::other("gone"),
    };
    assert_refused(
        &start,
        Code::PluginNotStarted,
        &json!({"plugin": "tasks"}),
        b"",
    );
}

#[test]
fn a_call_board_refuses_after_the_plugin_ran_carries_its_stderr_and_names_it_in_its_fields() {
    let log = b"a log line".to_vec();
    let logged = json!({"plugin": "tasks", "stderr": "a log line"});
    let slow = CallError::Timeout {
        plugin: tasks(),
        stderr: log.clone(),
    };
    assert_refused(&slow, Code::PluginTimeout, &logged, &log);
    // A plugin that ran and wrote nothing to stderr still names it.
    let large = CallError::TooLarge {
        plugin: tasks(),
        stderr: Vec::new(),
    };
    let quiet = json!({"plugin": "tasks", "stderr": ""});
    assert_refused(&large, Code::PluginTooLarge, &quiet, b"");
    let bad = CallError::BadOutput {
        plugin: tasks(),
        reason: BadOutput::Exit {
            exit: 0,
            written: "an error",
        },
        stderr: log.clone(),
    };
    assert_refused(&bad, Code::PluginBadOutput, &logged, &log);
    let failed = CallError::Failed {
        plugin: tasks(),
        exit: Some(3),
        stderr: log.clone(),
    };
    assert_refused(&failed, Code::PluginFailed, &logged, &log);
}

#[test]
fn an_ambiguous_capabilitys_hint_names_the_first_plugin_and_its_fields_name_them_all() {
    let refusal = CallError::Ambiguous {
        capability: "due".parse().unwrap(),
        plugins: names(&["notes", "tasks"]),
    }
    .refusal();
    assert_eq!(refusal.code, Board(Code::PluginAmbiguous));
    assert_eq!(
        refusal.hint.as_deref(),
        Some(
            "name the plugin as well, as in `board call notes.due`; `board plugin find --capability due` lists the plugins that offer it"
        )
    );
    assert_eq!(
        Value::Object(refusal.fields),
        json!({"capability": "due", "plugins": ["notes", "tasks"]})
    );
}

#[test]
fn a_plugins_own_refusal_keeps_its_code_and_fields_with_its_stderr_and_no_hint_from_board() {
    let tasks: PluginName = "tasks".parse().unwrap();
    let error = br#"{"$type": "board/error/v1", "code": "tasks::date::bad", "message": "no such date", "fields": {"date": "x"}}"#;
    let own = |stderr: &[u8]| {
        let Ok(Reply::Error(failure)) = protocol::reply::<Value>(&tasks, error, CALL_RESULT_TYPE)
        else {
            panic!("the fixture is a plugin's error document");
        };
        CallError::Plugin {
            plugin: tasks.clone(),
            failure,
            stderr: stderr.to_vec(),
        }
        .refusal()
    };
    let expected = |stderr: &str| Refusal {
        stderr: stderr.as_bytes().to_vec(),
        ..Refusal::plugin(
            PluginCode::parse(&tasks, "tasks::date::bad".to_owned()).unwrap(),
            "no such date".to_owned(),
            json!({"date": "x", "plugin": "tasks", "stderr": stderr})
                .as_object()
                .unwrap()
                .clone(),
        )
    };
    assert_eq!(own(b"a log line"), expected("a log line"));
    // A plugin that wrote nothing to stderr still names it, as Board's own
    // codes do.
    assert_eq!(own(b""), expected(""));
    // Board's refusal of the same run has Board's code and hint, and names
    // its stderr in its fields as well.
    let board = CallError::Failed {
        plugin: tasks,
        exit: Some(1),
        stderr: b"a log line".to_vec(),
    }
    .refusal();
    assert_eq!(board.code, Board(Code::PluginFailed));
    assert!(board.hint.is_some());
    assert_eq!(board.stderr, b"a log line");
    assert_eq!(board.fields["stderr"], "a log line");
}

#[test]
fn every_variable_board_passes_a_plugin_is_one_a_capability_s_env_may_not_name() {
    let set = Some(OsString::from("x"));
    let full = Environment {
        path: set.clone(),
        home: set.clone(),
        lang: set.clone(),
        tz: set.clone(),
        cache: set.clone(),
        data: set.clone(),
        config: set.clone(),
        timings: set.clone(),
        caller: set.clone(),
        day_start: set,
        ..Environment::default()
    };
    for (key, _) in environment(&full, &[], 1, &tasks()) {
        assert!(
            crate::manifest::PASSED.contains(&key),
            "{key} is passed to every plugin but a capability's env may name it"
        );
    }
}
