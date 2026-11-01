//! A built-in plugin's call is the `board call` a process plugin runs:
//! one call deeper, as the same plugin, with only its capability's declared
//! variables. Its `board` is the one `--personal` or `-C` opens.

use std::path::Path;

use super::*;
use crate::KnownBoards;
use crate::host::Elsewhere;

/// `outer`, built in, calling `fixture.due` with `input` through its host,
/// or through the board `elsewhere` names, answering with the output or
/// with the call's refusal.
fn calling(input: &'static str, elsewhere: Option<Elsewhere>) -> Fixture {
    answering(
        "outer",
        Box::new(move |host| {
            let refused =
                |command, refusal: &Refusal| PluginError::board_refused("outer", command, refusal);
            let other;
            let host = match &elsewhere {
                Some(elsewhere) => {
                    other = host
                        .board(elsewhere)
                        .map_err(|refusal| refused("--personal", &refusal))?;
                    &*other
                }
                None => host,
            };
            let input = serde_json::from_str(input).unwrap();
            match host.call(&target("fixture.due"), &input) {
                Ok(report) => Ok(report.output),
                Err(refusal) => Err(refused("call", &refusal)),
            }
        }),
    )
}

/// The case `outer.due` makes in `board`, the board and how `fixture`
/// answers, when the call starts at `depth` and `fixture` comes from
/// `source`.
#[derive(Clone, Copy, Debug)]
struct Nested {
    enabled: &'static [&'static str],
    input: &'static str,
    depth: &'static str,
    reply: Reply,
}

const NESTED: Nested = Nested {
    enabled: &["outer", "fixture"],
    input: r#"{"n": 1}"#,
    depth: "0",
    reply: Reply::Output(r#"{"count": 2}"#),
};

const BOTH: &[(&str, bool)] = &[("outer", true), ("fixture", true)];

impl Nested {
    fn call(self, source: Source) -> Result<Value, Box<Refusal>> {
        let scenario = board(BOTH, self.enabled, Holds::Same);
        let data = Scenario::new();
        let mut plugins = vec![calling(self.input, None)];
        match source {
            Source::BuiltIn => plugins.push(answering("fixture", self.reply.answer())),
            Source::Process => install(&data, "fixture", true, self.reply),
        }
        let mut board = open(&scenario);
        board.build_in(built(plugins));
        let directory = data.workdir().join("plugins");
        let plugins = Plugins {
            directory: Some(&directory),
            environment: environment(self.depth),
            ..Plugins::default()
        };
        (board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins))
            .map(|report| report.output)
            .map_err(|error| Box::new(error.refusal()))
    }
}

/// Each refusal of a nested call: Board's code, the refused case and its
/// admitted twin.
const REFUSALS: [(&str, Nested, Nested); 5] = [
    (
        "board::plugin::too_deep",
        Nested {
            depth: "7",
            ..NESTED
        },
        Nested {
            depth: "6",
            ..NESTED
        },
    ),
    (
        "board::plugin::not_enabled",
        Nested {
            enabled: &["outer"],
            ..NESTED
        },
        NESTED,
    ),
    (
        "board::plugin::bad_input",
        Nested {
            input: r#"{"n": "one"}"#,
            ..NESTED
        },
        NESTED,
    ),
    (
        "board::plugin::bad_output",
        Nested {
            reply: Reply::Output(r#"{"count": "two"}"#),
            ..NESTED
        },
        NESTED,
    ),
    (
        "fixture::task::late",
        Nested {
            reply: Reply::Error("fixture::task::late"),
            ..NESTED
        },
        NESTED,
    ),
];

#[test]
fn a_built_ins_call_is_refused_with_boards_code_as_board_call_refuses_it_and_its_twin_answers() {
    for source in [Source::BuiltIn, Source::Process] {
        for (code, refused, admitted) in REFUSALS {
            let refusal = refused.call(source).unwrap_err();
            assert_eq!(
                refusal.code.as_str(),
                "outer::board::refused",
                "{source:?} {refused:?}"
            );
            assert_eq!(
                refusal.fields["code"],
                json!(code),
                "{source:?} {refused:?}"
            );
            let start = format!("`board call` refused with {code}: ");
            assert!(refusal.message.starts_with(&start), "{}", refusal.message);
            let output = admitted
                .call(source)
                .unwrap_or_else(|refusal| panic!("{source:?} {admitted:?}: {}", refusal.message));
            assert_eq!(output, json!({"count": 2}), "{source:?} {admitted:?}");
        }
    }
}

/// A process `fixture` that declares `FIXTURE_HIDDEN` too, which `outer`
/// does not, and answers with the variables it was given.
fn telling(data: &Scenario) {
    install(data, "fixture", true, BASE.reply);
    let mut declared = manifest("fixture");
    declared["run"] = json!(["sh", "./run"]);
    declared["capabilities"][0]["env"] = json!(["FIXTURE_GIVEN", "FIXTURE_HIDDEN"]);
    data.write(
        "plugins/fixture/plugin.json",
        Item::File(&declared.to_string()),
    );
    let script = "#!/bin/sh\ncat > /dev/null\nprintf '{\"$type\": \"board/call-result/v1\", \"output\": {\"count\": 2, \"seen\": \"%s\"}}' \"$FIXTURE_GIVEN-$FIXTURE_HIDDEN-$BOARD_PLUGIN-$BOARD_CALL_DEPTH\"\n";
    data.write("plugins/fixture/run", Item::File(script));
}

#[cfg(unix)]
#[test]
fn a_process_plugin_a_built_in_calls_is_one_call_deeper_names_the_outer_plugin_and_sees_only_declared_variables()
 {
    let scenario = board(BOTH, NESTED.enabled, Holds::Same);
    let data = Scenario::new();
    telling(&data);
    let directory = data.workdir().join("plugins");
    let inherited = [("FIXTURE_GIVEN", "yes"), ("FIXTURE_HIDDEN", "no")];
    for (inside, seen) in [(None, "yes--outer-2"), (Some("first"), "yes--first-2")] {
        let mut board = open(&scenario);
        board.build_in(built(vec![calling(NESTED.input, None)]));
        let plugins = Plugins {
            directory: Some(&directory),
            environment: Environment {
                plugin: inside.map(OsString::from),
                inherited: (inherited.into_iter())
                    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                    .collect(),
                ..environment("0")
            },
            ..Plugins::default()
        };
        let output = board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins);
        assert_eq!(
            output.unwrap().output,
            json!({"count": 2, "seen": seen}),
            "{inside:?}"
        );
    }
}

/// A known boards file naming `personal` as the personal board.
fn known(personal: &Path) -> KnownBoards {
    let file = json!({"personal": personal}).to_string().into_bytes();
    KnownBoards::parse(Path::new("/known/config.json"), Ok(file)).unwrap()
}

/// `outer.due` in a board that enables only `outer`, calling `fixture.due`
/// in the board `elsewhere` names, with `known` the known boards.
fn elsewhere(elsewhere: Elsewhere, known: Option<&KnownBoards>) -> Result<Value, Box<Refusal>> {
    let scenario = board(BOTH, &["outer"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![
        calling(NESTED.input, Some(elsewhere)),
        fixture("fixture"),
    ]));
    let plugins = Plugins {
        environment: environment("0"),
        known,
        ..Plugins::default()
    };
    (board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins))
        .map(|report| report.output)
        .map_err(|error| Box::new(error.refusal()))
}

#[test]
fn a_built_ins_personal_board_is_refused_when_the_known_boards_name_none_and_its_twin_calls_there()
{
    let personal = board(BOTH, NESTED.enabled, Holds::Same);
    let refusal = elsewhere(Elsewhere::Personal, None).unwrap_err();
    assert_eq!(refusal.code.as_str(), "outer::board::refused");
    assert_eq!(refusal.fields["code"], json!("board::config::not_found"));
    let named = known(personal.workdir());
    let output = elsewhere(Elsewhere::Personal, Some(&named)).unwrap();
    assert_eq!(output, json!({"count": 2}));
}

#[test]
fn a_built_ins_call_in_its_own_board_is_refused_where_that_board_does_not_enable_it_and_its_twin_elsewhere_answers()
 {
    let personal = board(BOTH, NESTED.enabled, Holds::Same);
    let scenario = board(BOTH, &["outer"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![calling(NESTED.input, None), fixture("fixture")]));
    let named = known(personal.workdir());
    let plugins = Plugins {
        known: Some(&named),
        ..Plugins::default()
    };
    let refusal = (board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins))
        .unwrap_err()
        .refusal();
    assert_eq!(refusal.fields["code"], json!("board::plugin::not_enabled"));
    let output = elsewhere(Elsewhere::Personal, Some(&named)).unwrap();
    assert_eq!(output, json!({"count": 2}));
}

#[test]
fn a_built_ins_board_in_a_directory_that_does_not_exist_is_refused_as_minus_c_refuses_it_and_one_that_exists_is_opened()
 {
    let other = board(BOTH, NESTED.enabled, Holds::Same);
    let root = other.workdir().to_path_buf();
    let refusal = elsewhere(Elsewhere::Directory(root.join("missing")), None).unwrap_err();
    assert_eq!(refusal.fields["code"], json!("board::cli::usage"));
    assert!(
        refusal.message.contains("does not exist"),
        "{}",
        refusal.message
    );
    let output = elsewhere(Elsewhere::Directory(root), None).unwrap();
    assert_eq!(output, json!({"count": 2}));
}
