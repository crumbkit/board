//! A built-in plugin's put is `board put --batch` run inside the plugin:
//! the same checks, the same refusals and one commit, with no child process.

use super::*;
use crate::{Asker, PutError, PutReport};

/// A note, which needs a title.
const NOTE_SCHEMA: &str =
    r#"{"type": "object", "required": ["title"], "properties": {"title": {"type": "string"}}}"#;

/// The board `BASE` calls, committing as an identity of its own, with
/// notes and a note at `/notes/held`.
fn notes() -> Scenario {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    scenario
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        )
        .write(
            ".board/types/fixture/note/v1.schema.json",
            Item::File(NOTE_SCHEMA),
        )
        .write("notes/held.json", Item::File(HELD));
    scenario
}

const HELD: &str = r#"{"$type": "fixture/note/v1", "title": "held"}"#;

/// A batch line putting a note titled `title` at `path`, or one without a
/// title when `title` is `None`.
fn note(path: &str, title: Option<&str>) -> Value {
    let mut record = json!({"$type": "fixture/note/v1"});
    if let Some(title) = title {
        record["title"] = json!(title);
    }
    json!({"path": path, "record": record})
}

/// `line` with `expect` set.
fn expecting(mut line: Value, expect: &str) -> Value {
    line["expect"] = json!(expect);
    line
}

/// `fixture` putting `lines` through its host, answering with how many
/// files it wrote and the commit, or with the put's refusal.
fn putting(lines: Vec<Value>) -> Fixture {
    answering(
        "fixture",
        Box::new(move |host| match host.put(&lines) {
            Ok(report) => Ok(json!({
                "count": report.writes.len(),
                "commit": report.commit.map(|commit| commit.to_string()),
            })),
            Err(error) => Err(PluginError::board_refused(
                "fixture",
                "put --batch",
                &error.refusal(),
            )),
        }),
    )
}

/// Calls `fixture.due` putting `lines` in `scenario`, under `BOARD_PLUGIN`
/// when `inside` names a plugin, and checks no process started.
fn hosted(
    scenario: &Scenario,
    lines: &[Value],
    inside: Option<&str>,
) -> Result<Value, Box<Refusal>> {
    let mut board = open(scenario);
    board.build_in(built(vec![putting(lines.to_vec())]));
    let plugins = Plugins {
        environment: Environment {
            plugin: inside.map(OsString::from),
            ..Environment::default()
        },
        ..Plugins::default()
    };
    let called = call_base(&board, &plugins);
    assert_eq!(board.take_trace().work.get("started"), Some(&0));
    called
        .map(|report| report.output)
        .map_err(|error| Box::new(error.refusal()))
}

/// `lines` put with `Board::put_batch` as `plugin`, or as the owner.
fn batch(
    scenario: &Scenario,
    lines: &[Value],
    plugin: Option<&str>,
) -> Result<PutReport, PutError> {
    let mut board = open(scenario);
    board.ask_as(Asker {
        plugin: plugin.map(|name| name.parse().unwrap()),
        ..Asker::default()
    });
    let text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    board.put_batch(&mut text.as_bytes())
}

/// The code of each line `refusal` refuses.
fn line_codes(refusal: &Refusal) -> Vec<&str> {
    refusal.fields["refusals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["code"].as_str().unwrap())
        .collect()
}

fn exists(scenario: &Scenario, file: &str) -> bool {
    scenario.workdir().join(file).exists()
}

/// One guarantee of `put --batch`: a refused batch, the code of each line
/// it refuses, and an otherwise identical batch that is written.
struct Twin {
    case: &'static str,
    refused: Vec<Value>,
    codes: Vec<&'static str>,
    admitted: Vec<Value>,
}

/// Each guarantee, where `/notes/held` holds the blob `blob`.
fn twins(blob: &str) -> Vec<Twin> {
    let titled = |path| note(path, Some("t"));
    let stale = || vec!["board::put::stale"];
    vec![
        Twin {
            case: "the schema refuses a note without a title",
            refused: vec![note("/notes/a", None)],
            codes: vec!["board::record::invalid"],
            admitted: vec![titled("/notes/a")],
        },
        Twin {
            case: "only the owner writes under effects/",
            refused: vec![titled("/effects/a")],
            codes: vec!["board::settings::from_plugin"],
            admitted: vec![titled("/notes/a")],
        },
        Twin {
            case: "expect none finds a file",
            refused: vec![expecting(titled("/notes/held"), "none")],
            codes: stale(),
            admitted: vec![expecting(titled("/notes/a"), "none")],
        },
        Twin {
            case: "expect names another blob",
            refused: vec![expecting(titled("/notes/held"), &"0".repeat(40))],
            codes: stale(),
            admitted: vec![expecting(titled("/notes/held"), blob)],
        },
        Twin {
            case: "one refused line refuses the batch",
            refused: vec![titled("/notes/a"), note("/notes/b", None)],
            codes: vec!["board::record::invalid"],
            admitted: vec![titled("/notes/a"), titled("/notes/b")],
        },
    ]
}

#[test]
fn a_built_ins_put_refuses_as_put_batch_does_inside_the_plugin_and_writes_nothing_and_its_twin_is_written_in_one_commit()
 {
    let blob = notes().blob(HELD).to_string();
    for Twin {
        case,
        refused,
        codes,
        admitted,
    } in twins(&blob)
    {
        let scenario = notes();
        let before = scenario.head().map(|head| head.id);
        let refusal = hosted(&scenario, &refused, None).unwrap_err();
        assert_eq!(refusal.code.as_str(), "fixture::board::refused", "{case}");
        assert_eq!(line_codes(&refusal), codes, "{case}");
        let expected = batch(&scenario, &refused, Some("fixture"))
            .unwrap_err()
            .refusal();
        assert_eq!(
            (&refusal.fields["code"], &refusal.fields["refusals"]),
            (&json!(expected.code.as_str()), &expected.fields["refusals"]),
            "{case}"
        );
        assert_eq!(scenario.head().map(|head| head.id), before, "{case}");
        for line in refused.iter().filter(|line| line["path"] != "/notes/held") {
            let file = format!("{}.json", &line["path"].as_str().unwrap()[1..]);
            assert!(!exists(&scenario, &file), "{case}: {file}");
        }
        assert_eq!(
            std::fs::read_to_string(scenario.workdir().join("notes/held.json")).unwrap(),
            HELD,
            "{case}"
        );

        let scenario = notes();
        let before = scenario.head().map(|head| head.id);
        let output = hosted(&scenario, &admitted, None).unwrap();
        assert_eq!(output["count"], json!(admitted.len()), "{case}");
        let head = scenario.head().unwrap();
        assert_eq!(output["commit"], json!(head.id.to_string()), "{case}");
        assert_eq!(head.parents, Vec::from_iter(before), "{case}: one commit");
        for line in &admitted {
            let file = format!("{}.json", &line["path"].as_str().unwrap()[1..]);
            assert!(head.files.contains_key(&file), "{case}: {file}");
        }
    }
}

#[test]
fn a_built_ins_put_is_checked_as_the_plugin_and_the_owners_same_put_is_written() {
    let line = [note("/effects/a", Some("t"))];
    let scenario = notes();
    let refusal = hosted(&scenario, &line, None).unwrap_err();
    assert_eq!(line_codes(&refusal), ["board::settings::from_plugin"]);
    assert!(batch(&scenario, &line, None).is_ok());
    assert!(exists(&scenario, "effects/a.json"));
}

#[test]
fn a_built_ins_put_under_board_plugin_names_that_plugin_and_without_it_names_the_called_one() {
    let line = [note("/effects/a", Some("t"))];
    for (inside, named) in [(Some("outer"), "outer"), (None, "fixture")] {
        let refusal = hosted(&notes(), &line, inside).unwrap_err();
        assert_eq!(
            refusal.fields["refusals"][0]["fields"]["plugin"],
            json!(named),
            "{inside:?}"
        );
    }
}

#[test]
fn a_built_ins_put_under_a_board_plugin_that_names_no_plugin_is_refused_as_invalid_and_one_that_names_a_plugin_is_written()
 {
    let line = [note("/notes/a", Some("t"))];
    let refusal = hosted(&notes(), &line, Some("Not A Plugin")).unwrap_err();
    assert_eq!(refusal.fields["code"], json!("board::config::invalid"));
    let output = hosted(&notes(), &line, Some("outer")).unwrap();
    assert_eq!(output["count"], json!(1));
}
