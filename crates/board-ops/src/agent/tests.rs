//! An agent's variable is read, its answer is found, and each way it fails
//! is refused in the plugin's namespace naming the variable.

use std::path::PathBuf;
use std::time::SystemTime;

use board_model::LogicalPath;
use serde_json::json;

use super::*;
use crate::Limit;
use crate::host::{Settings, Variables};
use crate::query::{QueryError, QueryReport};
use crate::read::{GetReport, ReadError};
use crate::refs::{Absent, RefsError, RefsReport};

/// A host that only runs processes, with `PATH` set.
struct Bare(Settings);

impl Host for Bare {
    fn query(&self, _: &str, _: Limit) -> Result<QueryReport, QueryError> {
        unreachable!("an agent queries nothing")
    }

    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        unreachable!("an agent resolves nothing")
    }

    fn get_each(&self, _: &[LogicalPath]) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        unreachable!("an agent reads nothing")
    }

    fn put(&self, _: &[serde_json::Value]) -> Result<crate::PutReport, crate::PutError> {
        unreachable!("an agent writes nothing")
    }

    fn settings(&self) -> &Settings {
        &self.0
    }
}

fn host() -> Bare {
    Bare(Settings {
        now: SystemTime::UNIX_EPOCH,
        day_start: None,
        caller: None,
        environment: Variables(vec![("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]),
        cache_home: None::<PathBuf>,
    })
}

fn example(argv: &[&str]) -> Agent {
    Agent {
        plugin: "example".to_owned(),
        variable: "EXAMPLE_AGENT".to_owned(),
        argv: argv.iter().map(|part| (*part).to_owned()).collect(),
    }
}

fn ask(agent: &Agent, prompt: &str, seconds: f64) -> Result<String, PluginError> {
    agent.ask(
        &host(),
        prompt,
        Duration::from_secs_f64(seconds),
        "shorten it",
    )
}

#[test]
fn an_agent_past_its_time_is_refused_with_the_hint_and_one_within_it_answers_its_prompt() {
    let refused = ask(&example(&["sh", "-c", "sleep 6"]), "", 0.2).unwrap_err();
    assert_eq!(refused.code, "example::agent::timeout");
    assert!(
        (refused.message)
            .contains("shorten it, or make the agent EXAMPLE_AGENT names answer sooner"),
        "{}",
        refused.message
    );
    assert_eq!(
        ask(&example(&["cat"]), "the prompt\n", 5.0).unwrap(),
        "the prompt\n"
    );
}

#[test]
fn an_agent_that_cannot_start_is_refused_naming_its_variable_and_program_and_one_that_can_runs() {
    let refused = ask(
        &example(&["/nonexistent/agent", "--key", "secret"]),
        "",
        5.0,
    )
    .unwrap_err();
    assert_eq!(refused.code, "example::agent::not_started");
    assert_eq!(refused.fields["program"], json!("/nonexistent/agent"));
    let shown = format!("{} {:?}", refused.message, refused.fields);
    assert!(!shown.contains("secret"), "{shown}");
    assert!(
        refused.message.contains("change EXAMPLE_AGENT"),
        "{}",
        refused.message
    );
    assert_eq!(
        ask(&example(&["sh", "-c", "echo ran"]), "", 5.0).unwrap(),
        "ran\n"
    );
}

#[test]
fn an_agent_that_exits_non_zero_is_refused_with_its_output_and_one_that_exits_zero_answers() {
    let refused = ask(
        &example(&["sh", "-c", "echo out; echo err >&2; exit 3"]),
        "",
        5.0,
    );
    let refused = refused.unwrap_err();
    assert_eq!(refused.code, "example::agent::failed");
    assert_eq!(
        (
            &refused.fields["exit"],
            &refused.fields["stdout"],
            &refused.fields["stderr"]
        ),
        (&json!(3), &json!("out\n"), &json!("err\n"))
    );
    assert_eq!(
        ask(&example(&["sh", "-c", "echo out; exit 0"]), "", 5.0).unwrap(),
        "out\n"
    );
}

#[test]
fn an_unset_or_malformed_variable_is_refused_and_an_array_naming_a_program_is_read() {
    let named = |value: Option<&str>| {
        Agent::named(
            "example",
            "EXAMPLE_AGENT",
            value.map(OsString::from).as_ref(),
        )
    };
    assert_eq!(named(None).unwrap_err().code, "example::agent::not_set");
    for value in [
        "claude -p",
        "[]",
        "[\"\"]",
        "[\"claude\", 1]",
        "[\"x\", \"secret\"",
    ] {
        let refused = named(Some(value)).unwrap_err();
        assert_eq!(refused.code, "example::agent::invalid", "{value}");
        assert!(!refused.message.contains("secret"), "{}", refused.message);
    }
    let agent = named(Some(r#"["claude", "--tools", ""]"#)).unwrap();
    assert_eq!(agent.argv, ["claude", "--tools", ""]);
}

#[test]
fn the_answer_is_an_envelope_s_result_or_the_whole_stdout_and_the_last_object_with_the_key_is_found()
 {
    let envelope = json!({"type": "result", "result": "Done.\n{\"findings\": []}"}).to_string();
    assert_eq!(answer(&envelope), "Done.\n{\"findings\": []}");
    assert_eq!(answer("plain {\"a\": 1}"), "plain {\"a\": 1}");
    let failed = json!({"is_error": true, "result": "{\"findings\": []}"}).to_string();
    assert_eq!(answer(&failed), failed);
    let text = r#"{"findings": [1]} then {"findings": [2], "x": {"y": 1}} and {"other": 3}"#;
    assert_eq!(
        last_object(text, "findings").unwrap()["findings"],
        json!([2])
    );
    assert_eq!(last_object("no object here", "findings"), None);
}

#[test]
fn a_tail_starts_on_a_character_and_a_short_text_is_whole() {
    assert_eq!(tail("abc", 10), "abc");
    assert_eq!(tail("abcdef", 2), "ef");
    // `é` is two bytes, so a cut inside it moves to the next character.
    assert_eq!(tail("aéb", 2), "b");
}
