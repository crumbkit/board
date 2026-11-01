//! A built-in plugin is called as an installed one is: the same choice, the
//! same checks and the same codes, without a child process.

use std::ffi::OsString;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use board_model::{Code, PluginName, RefusalCode};
use board_plugin::{CallReport, Environment, Limits, Target};
use board_scenario::{Item, Scenario};
use serde_json::json;

use super::*;
use crate::tests::open;
use crate::{CallFailed, Plugins};

/// Takes a number.
const INPUT_SCHEMA: &str =
    r#"{"type": "object", "required": ["n"], "properties": {"n": {"type": "integer"}}}"#;
/// Answers a count.
const OUTPUT_SCHEMA: &str =
    r#"{"type": "object", "required": ["count"], "properties": {"count": {"type": "integer"}}}"#;
/// The schema each fixture ships for its one type.
const ITEM_SCHEMA: &str = r#"{"type": "object"}"#;

/// How a fixture plugin answers every call.
type Answer = Box<dyn Fn(&dyn Host) -> Result<Value, PluginError> + Sync>;

/// A built-in plugin offering `due` and shipping `<name>/item/v1`, or not
/// shipping its schema when `ships` is false.
struct Fixture {
    name: &'static str,
    ships: bool,
    answer: Answer,
}

impl Plugin for Fixture {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest(self.name)).unwrap()
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let shipped = (item(self.name), serde_json::from_str(ITEM_SCHEMA).unwrap());
        if self.ships { vec![shipped] } else { vec![] }
    }

    fn call(
        &self,
        host: &dyn Host,
        _capability: &CapabilityName,
        _input: Value,
    ) -> Result<Value, PluginError> {
        (self.answer)(host)
    }
}

fn item(plugin: &str) -> RecordType {
    format!("{plugin}/item/v1").parse().unwrap()
}

/// The manifest of the fixture named `name`, without `run`.
fn manifest(name: &str) -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": name,
        "version": "0.1.0",
        "description": "",
        "types": [item(name).as_str()],
        "capabilities": [{
            "name": "due",
            "description": "",
            "inputSchema": serde_json::from_str::<Value>(INPUT_SCHEMA).unwrap(),
            "outputSchema": serde_json::from_str::<Value>(OUTPUT_SCHEMA).unwrap(),
            "env": ["FIXTURE_GIVEN"],
        }],
    })
}

/// What a fixture answers: an output, or an error with this code.
#[derive(Clone, Copy, Debug)]
enum Reply {
    Output(&'static str),
    Error(&'static str),
}

impl Reply {
    fn answer(self) -> Answer {
        Box::new(move |_| match self {
            Self::Output(output) => Ok(serde_json::from_str(output).unwrap()),
            Self::Error(code) => Err(PluginError {
                code: code.to_owned(),
                message: "the task is late".to_owned(),
                fields: Map::new(),
            }),
        })
    }

    /// The same answer from a process: a document on stdout, and an exit.
    fn script(self) -> String {
        let (stdout, exit) = match self {
            Self::Output(output) => (
                format!(r#"{{"$type": "board/call-result/v1", "output": {output}}}"#),
                0,
            ),
            Self::Error(code) => (
                format!(
                    r#"{{"$type": "board/error/v1", "code": "{code}", "message": "the task is late"}}"#
                ),
                1,
            ),
        };
        format!("#!/bin/sh\ncat > /dev/null\nprintf '%s' '{stdout}'\nexit {exit}\n")
    }
}

/// Where a case's plugins come from.
#[derive(Clone, Copy, Debug)]
enum Source {
    BuiltIn,
    Process,
}

/// How the board holds each plugin's shipped schema.
#[derive(Clone, Copy, Debug)]
enum Holds {
    Same,
    Missing,
    Differs,
}

/// One call: the plugins there are, each named with whether it ships its
/// schema, the board that enables some, and what is asked.
#[derive(Clone, Copy, Debug)]
struct Case {
    plugins: &'static [(&'static str, bool)],
    enabled: &'static [&'static str],
    holds: Holds,
    target: &'static str,
    input: &'static str,
    depth: &'static str,
    reply: Reply,
}

const BASE: Case = Case {
    plugins: &[("fixture", true)],
    enabled: &["fixture"],
    holds: Holds::Same,
    target: "fixture.due",
    input: r#"{"n": 1}"#,
    depth: "0",
    reply: Reply::Output(r#"{"count": 2}"#),
};

/// A board enabling `enabled` and holding each of `plugins`' schemas as
/// `holds` says.
fn board(plugins: &[(&str, bool)], enabled: &[&str], holds: Holds) -> Scenario {
    let board = Scenario::new();
    board.write(
        ".board/board.json",
        Item::File(&json!({"$type": "board/config/v1", "plugins": enabled}).to_string()),
    );
    for (name, _) in plugins {
        let held = match holds {
            Holds::Same => ITEM_SCHEMA,
            Holds::Differs => r#"{"type": "array"}"#,
            Holds::Missing => continue,
        };
        board.write(&item(name).schema_path(), Item::File(held));
    }
    board
}

/// Installs `name` in `data`'s plugins directory, answering `reply`.
fn install(data: &Scenario, name: &str, ships: bool, reply: Reply) {
    let mut manifest = manifest(name);
    manifest["run"] = json!(["sh", "./run"]);
    data.write(
        &format!("plugins/{name}/plugin.json"),
        Item::File(&manifest.to_string()),
    )
    .write(&format!("plugins/{name}/run"), Item::File(&reply.script()));
    if ships {
        data.write(
            &format!("plugins/{name}/types/{}.schema.json", item(name).as_str()),
            Item::File(ITEM_SCHEMA),
        );
    }
}

/// `plugins` built into one Board.
fn built(plugins: Vec<Fixture>) -> &'static [&'static dyn Plugin] {
    let plugins: Vec<&'static dyn Plugin> = plugins
        .into_iter()
        .map(|plugin| &*Box::leak(Box::new(plugin)) as &'static dyn Plugin)
        .collect();
    Box::leak(plugins.into_boxed_slice())
}

fn target(text: &str) -> Target {
    match text.strip_prefix("--capability ") {
        Some(capability) => Target::Capability(capability.parse().unwrap()),
        None => text.parse().unwrap(),
    }
}

fn environment(depth: &str) -> Environment {
    Environment {
        path: Some(OsString::from("/usr/bin:/bin")),
        depth: Some(OsString::from(depth)),
        ..Environment::default()
    }
}

impl Case {
    /// Makes this call with the plugins built in or installed.
    fn call(self, source: Source) -> Result<CallReport, CallFailed> {
        let scenario = board(self.plugins, self.enabled, self.holds);
        let data = Scenario::new();
        let mut fixtures = Vec::new();
        for &(name, ships) in self.plugins {
            match source {
                Source::BuiltIn => fixtures.push(Fixture {
                    name,
                    ships,
                    answer: self.reply.answer(),
                }),
                Source::Process => install(&data, name, ships, self.reply),
            }
        }
        let mut board = open(&scenario);
        board.build_in(built(fixtures));
        let directory = data.workdir().join("plugins");
        let plugins = Plugins {
            directory: Some(&directory),
            environment: environment(self.depth),
            ..Plugins::default()
        };
        board.call(&target(self.target), self.input.as_bytes(), &plugins)
    }
}

/// Two plugins offering `due`.
const OTHER: &[(&str, bool)] = &[("fixture", true), ("other", true)];

/// Each refusal a call can give without starting a process: its code, the
/// refused call, and an admitted twin.
const CASES: [(&str, Case, Case); 15] = [
    (
        "board::plugin::too_deep",
        Case { depth: "8", ..BASE },
        Case { depth: "7", ..BASE },
    ),
    (
        "board::plugin::not_installed",
        Case {
            target: "missing.due",
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::not_installed",
        Case {
            target: "fixture.overdue",
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::not_installed",
        Case {
            target: "--capability overdue",
            ..BASE
        },
        Case {
            target: "--capability due",
            ..BASE
        },
    ),
    (
        "board::plugin::not_enabled",
        Case {
            enabled: &[],
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::ambiguous",
        Case {
            plugins: OTHER,
            enabled: &["fixture", "other"],
            target: "--capability due",
            ..BASE
        },
        Case {
            plugins: OTHER,
            target: "--capability due",
            ..BASE
        },
    ),
    (
        "board::plugin::schema_missing",
        Case {
            holds: Holds::Missing,
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::schema_differs",
        Case {
            holds: Holds::Differs,
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::bad_input",
        Case {
            input: r#"{"n": "one"}"#,
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::bad_input",
        Case {
            input: r#"{"n": 1"#,
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::bad_output",
        Case {
            reply: Reply::Output(r#"{"count": "two"}"#),
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::bad_output",
        Case {
            reply: Reply::Error("other::task::late"),
            ..BASE
        },
        BASE,
    ),
    (
        "fixture::task::late",
        Case {
            reply: Reply::Error("fixture::task::late"),
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::bad_manifest",
        Case {
            plugins: &[("fixture", false)],
            ..BASE
        },
        BASE,
    ),
    (
        "board::plugin::not_enabled",
        Case {
            plugins: OTHER,
            enabled: &["other"],
            target: "fixture.due",
            ..BASE
        },
        Case {
            plugins: OTHER,
            enabled: &["other", "fixture"],
            ..BASE
        },
    ),
];

#[test]
fn every_refusal_without_a_process_has_the_same_code_built_in_or_installed() {
    for source in [Source::BuiltIn, Source::Process] {
        for (code, refused, admitted) in CASES {
            let refusal = refused.call(source).unwrap_err().refusal();
            assert_eq!(refusal.code.as_str(), code, "{source:?} {refused:?}");
            let report = admitted.call(source).unwrap_or_else(|error| {
                panic!("{source:?} {admitted:?}: {}", error.refusal().message)
            });
            assert_eq!(
                report.output,
                json!({"count": 2}),
                "{source:?} {admitted:?}"
            );
            assert_eq!(report.plugin.as_str(), "fixture");
        }
    }
}

/// The fixture named `name`, answering as `BASE` does.
fn fixture(name: &'static str) -> Fixture {
    answering(name, BASE.reply.answer())
}

fn answering(name: &'static str, answer: Answer) -> Fixture {
    Fixture {
        name,
        ships: true,
        answer,
    }
}

/// `board` with `plugins` built in, calling `fixture.due` as `BASE` does.
fn call_base(board: &Board, plugins: &Plugins<'_>) -> Result<CallReport, CallFailed> {
    board.call(&target(BASE.target), BASE.input.as_bytes(), plugins)
}

#[test]
fn a_built_in_capability_runs_in_board_with_no_child_process() {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![fixture("fixture")]));
    // No plugins directory, no PATH, and limits no process could meet.
    let plugins = Plugins {
        limits: Limits {
            time: Duration::ZERO,
            stdout: 0,
            stderr: 0,
        },
        ..Plugins::default()
    };
    let report = call_base(&board, &plugins).unwrap();
    assert_eq!(report.output, json!({"count": 2}));
    let trace = board.take_trace();
    assert_eq!(trace.work.get("started"), Some(&0));
    let phases: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
    assert!(phases.contains(&"plugin.call"), "{phases:?}");
    assert!(!phases.contains(&"plugin.reply"), "{phases:?}");
}

#[test]
fn a_plugin_both_built_in_and_installed_is_refused_and_one_installed_under_another_name_is_admitted()
 {
    for (installed, code) in [("fixture", Some(Code::PluginDuplicate)), ("other", None)] {
        let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
        let data = Scenario::new();
        install(&data, installed, true, BASE.reply);
        let mut board = open(&scenario);
        board.build_in(built(vec![fixture("fixture")]));
        let directory = data.workdir().join("plugins");
        let plugins = Plugins {
            directory: Some(&directory),
            environment: environment("0"),
            ..Plugins::default()
        };
        for asked in ["fixture.due", "--capability due"] {
            let called = board.call(&target(asked), BASE.input.as_bytes(), &plugins);
            assert_eq!(
                called.err().map(|error| error.refusal().code),
                code.map(RefusalCode::Board),
                "{installed} {asked}"
            );
        }
        let name: PluginName = "fixture".parse().unwrap();
        let enabled = board.plugin_enable(Some(&directory), None, &name, &[]);
        assert_eq!(enabled.err().map(|error| error.code()), code, "{installed}");
        let listed = board.plugin_list(Some(&directory)).unwrap();
        let entry = (listed.plugins.iter())
            .find(|entry| entry.name == "fixture")
            .unwrap();
        assert_eq!(entry.code(), code, "{installed}");
    }
}

#[test]
fn a_built_in_plugin_is_listed_found_and_enabled_as_an_installed_one_is() {
    let scenario = board(&[], &[], Holds::Same);
    scenario.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    let plugins = built(vec![fixture("fixture")]);
    let mut board = open(&scenario);
    board.build_in(plugins);
    let listed = board.plugin_list(None).unwrap();
    let [entry] = &listed.plugins[..] else {
        panic!("{listed:?}")
    };
    assert_eq!((entry.name.as_str(), entry.enabled), ("fixture", false));
    assert_eq!((entry.code(), &entry.shadows), (None, &None));
    let due = "due".parse().unwrap();
    let found = board.plugin_find(None, Some(&due), &[]).unwrap();
    assert_eq!(found.plugins.len(), 1);
    assert_eq!(
        call_base(&board, &Plugins::default())
            .unwrap_err()
            .refusal()
            .code,
        RefusalCode::Board(Code::PluginNotEnabled)
    );

    let report = (board.plugin_enable(None, None, &"fixture".parse().unwrap(), &[])).unwrap();
    assert_eq!(
        report.put.writes.len(),
        2,
        "the schema and the configuration"
    );
    let mut board = open(&scenario);
    board.build_in(plugins);
    let listed = board.plugin_list(None).unwrap();
    assert!(listed.plugins[0].enabled);
    assert_eq!(listed.plugins[0].code(), None);
    assert!(call_base(&board, &Plugins::default()).is_ok());
}

#[test]
fn a_row_limit_is_one_to_the_most_a_query_returns() {
    assert_eq!(Limit::rows(0), None);
    assert_eq!(Limit::rows(1), Some(Limit(1)));
    assert_eq!(Limit::rows(MAX_LIMIT), Some(Limit(MAX_LIMIT)));
    assert_eq!(Limit::rows(MAX_LIMIT + 1), None);
}

#[test]
fn a_built_in_queries_the_board_within_its_row_limit() {
    let counted = |limit: Limit| {
        let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
        scenario.write("notes/a.md", Item::File("a\n"));
        let mut board = open(&scenario);
        board.build_in(built(vec![answering(
            "fixture",
            Box::new(move |host| {
                let report = host.query("SELECT logical FROM entries", limit).unwrap();
                Ok(json!({"count": report.rows.len(), "truncated": report.truncated}))
            }),
        )]));
        call_base(&board, &Plugins::default()).unwrap().output
    };
    assert_eq!(
        counted(Limit::rows(1).unwrap()),
        json!({"count": 1, "truncated": true})
    );
    let all = counted(Limit::ALL);
    assert!(all["count"].as_u64().unwrap() > 1, "{all}");
    assert_eq!(all["truncated"], false);
}

#[test]
fn a_built_in_reads_its_settings_from_the_host_in_place_of_its_environment() {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![answering(
        "fixture",
        Box::new(|host| {
            let settings = host.settings();
            let now = settings.now.duration_since(UNIX_EPOCH).unwrap();
            Ok(json!({
                "count": now.as_secs(),
                "day_start": settings.day_start,
                "caller": settings.caller,
            }))
        }),
    )]));
    let seconds = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let before = seconds();
    let set = Plugins {
        environment: Environment {
            day_start: Some(OsString::from("07:00")),
            caller: Some(OsString::from("/work/repo")),
            ..Environment::default()
        },
        ..Plugins::default()
    };
    let output = call_base(&board, &set).unwrap().output;
    let now = output["count"].as_u64().unwrap();
    assert!((before..=seconds()).contains(&now), "{output}");
    assert_eq!(
        (&output["day_start"], &output["caller"]),
        (&json!("07:00"), &json!("/work/repo"))
    );
    let unset = call_base(&board, &Plugins::default()).unwrap().output;
    assert_eq!(
        (&unset["day_start"], &unset["caller"]),
        (&Value::Null, &Value::Null)
    );
}

#[cfg(unix)]
#[test]
fn a_built_in_runs_a_process_with_its_capabilitys_declared_variables_and_its_own_name_and_no_others()
 {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![answering(
        "fixture",
        Box::new(|host| {
            let arguments = [
                "-c".to_owned(),
                "printf '%s' \"$FIXTURE_GIVEN-$FIXTURE_HIDDEN-$BOARD_PLUGIN-$BOARD_CALL_DEPTH\""
                    .to_owned(),
            ];
            let process = Run {
                program: "sh",
                arguments: &arguments,
                directory: None,
                input: b"",
                deadline: Duration::from_secs(5),
            };
            let exited = host.run(&process).unwrap();
            Ok(json!({"count": exited.code, "seen": String::from_utf8(exited.stdout).unwrap()}))
        }),
    )]));
    let set = Plugins {
        environment: Environment {
            path: Some(OsString::from("/usr/bin:/bin")),
            inherited: [("FIXTURE_GIVEN", "yes"), ("FIXTURE_HIDDEN", "no")]
                .into_iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect(),
            ..Environment::default()
        },
        ..Plugins::default()
    };
    let output = call_base(&board, &set).unwrap().output;
    // The process is identified as the built-in plugin, one call deeper.
    assert_eq!(output, json!({"count": 0, "seen": "yes--fixture-1"}));
}

/// `fixture` answering the query `sql` with how many rows it returned and
/// whether its index was in memory, or with the query's refusal.
fn querying(sql: &'static str) -> Fixture {
    answering(
        "fixture",
        Box::new(move |host| match host.query(sql, Limit::ALL) {
            Ok(report) => Ok(
                json!({"count": report.rows.len(), "memory": report.memory.is_some(), "rows": report.rows}),
            ),
            Err(error) => Err(PluginError::board_refused(
                "fixture",
                "query",
                &error.refusal(),
            )),
        }),
    )
}

#[test]
fn a_failing_host_query_reaches_the_call_as_the_plugins_board_refused_with_boards_code_and_its_twin_answers()
 {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let bad = "SELECT logical FROM nowhere";
    let expected = (open(&scenario).query(bad, 1, None, None))
        .unwrap_err()
        .refusal();
    for (sql, refused) in [(bad, true), ("SELECT logical FROM entries", false)] {
        let mut board = open(&scenario);
        board.build_in(built(vec![querying(sql)]));
        let called = call_base(&board, &Plugins::default());
        if !refused {
            assert!(called.unwrap().output["count"].as_u64().unwrap() > 0);
            continue;
        }
        let refusal = called.unwrap_err().refusal();
        assert_eq!(refusal.code.as_str(), "fixture::board::refused");
        assert_eq!(refusal.fields["code"], json!(expected.code.as_str()));
        let start = format!("`board query` refused with {}: ", expected.code.as_str());
        assert!(refusal.message.starts_with(&start), "{}", refusal.message);
    }
}

#[test]
fn a_built_ins_queries_use_the_callers_index_cache_and_answer_as_a_cold_index_does() {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    scenario.write("notes/a.md", Item::File("a\n"));
    let cache = tempfile::tempdir().unwrap();
    let call = |cache: Option<&std::path::Path>| {
        let mut board = open(&scenario);
        board.build_in(built(vec![querying("SELECT logical FROM entries")]));
        let plugins = Plugins {
            cache,
            ..Plugins::default()
        };
        let output = call_base(&board, &plugins).unwrap().output;
        (output, board.take_trace().work.get("parsed").copied())
    };
    let (cold, _) = call(None);
    assert_eq!(
        cold["memory"], true,
        "without a cache the index is in memory"
    );
    let (first, parsed_first) = call(Some(cache.path()));
    assert!(
        parsed_first.unwrap_or(0) > 0,
        "the first cached call builds the index"
    );
    let (warm, parsed) = call(Some(cache.path()));
    assert_eq!(parsed.unwrap_or(0), 0, "a warm index parses no blob again");
    assert_eq!(
        (&first["memory"], &warm["memory"]),
        (&json!(false), &json!(false))
    );
    for answer in [&first, &warm] {
        assert_eq!(
            (&answer["count"], &answer["rows"]),
            (&cold["count"], &cold["rows"])
        );
    }
    assert!(cold["count"].as_u64().unwrap() > 0);
}

#[test]
fn plugin_find_leaves_out_a_built_in_plugin_installed_again_and_finds_one_installed_under_another_name()
 {
    for (installed, found) in [("fixture", 0), ("other", 2)] {
        let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
        let data = Scenario::new();
        install(&data, installed, true, BASE.reply);
        let mut board = open(&scenario);
        board.build_in(built(vec![fixture("fixture")]));
        let directory = data.workdir().join("plugins");
        let due = "due".parse().unwrap();
        let report = (board.plugin_find(Some(&directory), Some(&due), &[])).unwrap();
        assert_eq!(report.plugins.len(), found, "{installed}");
    }
}

#[test]
fn a_board_refused_error_adds_the_refusals_hint_and_leaves_out_an_empty_one() {
    let mut refusal = Refusal::new(
        Code::PathMalformed,
        "the path is bad",
        &board_model::Fields::default(),
    );
    refusal.hint = Some("give another path".to_owned());
    let hinted = PluginError::board_refused("fixture", "query", &refusal);
    refusal.hint = Some(String::new());
    let empty = PluginError::board_refused("fixture", "query", &refusal);
    let start = format!(
        "`board query` refused with {}: the path is bad",
        refusal.code.as_str()
    );
    assert_eq!(hinted.message, format!("{start}; give another path"));
    assert_eq!(empty.message, start);
}

#[test]
fn built_in_plugins_debug_as_their_names() {
    let plugins = built(vec![
        answering("first", BASE.reply.answer()),
        answering("second", BASE.reply.answer()),
    ]);
    assert_eq!(
        format!("{:?}", BuiltIn(plugins)),
        r#"[PluginName("first"), PluginName("second")]"#
    );
}

/// `fixture` answering with what `refs` and `get_each` give for
/// `/notes/a.md` and `/gone`: each path's display path, bytes or code.
fn resolving() -> Fixture {
    answering(
        "fixture",
        Box::new(|host| {
            let paths: Vec<LogicalPath> = ["/notes/a.md", "/gone"]
                .iter()
                .map(|path| path.parse().unwrap())
                .collect();
            let answered = |result: Result<String, Code>| match result {
                Ok(display) => json!(display),
                Err(code) => json!(code.as_str()),
            };
            let refs: Vec<Value> = (host.refs(&paths, Absent::Refuse).unwrap().into_iter())
                .map(|report| {
                    answered(
                        report
                            .map(|report| report.display.to_string())
                            .map_err(|error| error.code()),
                    )
                })
                .collect();
            let located = host.refs(&paths[1..], Absent::Locate).unwrap();
            let missing = located[0].as_ref().unwrap().missing.as_ref().unwrap();
            let read: Vec<Value> = (host.get_each(&paths).unwrap().into_iter())
                .map(|report| {
                    answered(
                        report
                            .map(|report| String::from_utf8(report.bytes).unwrap())
                            .map_err(|error| error.code()),
                    )
                })
                .collect();
            Ok(
                json!({"count": 0, "refs": refs, "missing": missing.display.to_string(), "read": read}),
            )
        }),
    )
}

#[test]
fn a_built_in_resolves_and_reads_paths_as_refs_and_get_do_and_a_missing_path_is_refused_or_located()
{
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    scenario.write("notes/a.md", Item::File("a\n"));
    let mut board = open(&scenario);
    board.build_in(built(vec![resolving()]));
    let output = call_base(&board, &Plugins::default()).unwrap().output;
    assert_eq!(
        output["refs"],
        json!(["/notes/a.md", "board::path::not_found"])
    );
    assert_eq!(output["missing"], json!("/gone"));
    assert_eq!(output["read"], json!(["a\n", "board::path::not_found"]));
}

mod put;
