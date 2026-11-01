//! An agent is stopped with everything it started, and its answer is read.

use std::time::{Duration, Instant};

use serde_json::json;

use super::*;

/// How long the agents these tests stop would otherwise sleep, in seconds.
const SLEEP: u64 = 6;

fn example(argv: &[&str]) -> Agent {
    Agent {
        plugin: "example".to_owned(),
        variable: "EXAMPLE_AGENT".to_owned(),
        argv: argv.iter().map(|part| (*part).to_owned()).collect(),
    }
}

fn path() -> Vec<(String, OsString)> {
    vec![("PATH".to_owned(), OsString::from("/usr/bin:/bin"))]
}

fn ask(agent: &Agent, prompt: &str, seconds: f64) -> Result<String, Refused> {
    agent.ask(
        &path(),
        prompt,
        Duration::from_secs_f64(seconds),
        "shorten it",
    )
}

/// Whether the process `pid` stops within five seconds.
fn gone(pid: i32) -> bool {
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while rustix::process::test_kill_process(pid).is_ok() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

#[test]
fn an_agent_past_its_time_is_stopped_and_one_within_it_answers() {
    let started = Instant::now();
    let sleeper = format!("sleep {SLEEP}; echo late");
    let refused = ask(&example(&["sh", "-c", &sleeper]), "", 0.2).unwrap_err();
    assert_eq!(refused.code, "example::agent::timeout");
    assert!(
        (refused.message)
            .contains("shorten it, or make the agent EXAMPLE_AGENT names answer sooner"),
        "{}",
        refused.message
    );
    assert!(started.elapsed() < Duration::from_secs(SLEEP / 2));
    assert_eq!(
        ask(&example(&["sh", "-c", "echo soon"]), "", 5.0).unwrap(),
        "soon\n"
    );
}

#[test]
fn an_agent_reads_its_prompt_on_stdin_in_an_empty_directory() {
    let answer = ask(
        &example(&["sh", "-c", "cat; ls -A | wc -l"]),
        "the prompt\n",
        5.0,
    );
    let answer = answer.unwrap();
    let mut lines = answer.lines();
    assert_eq!(lines.next(), Some("the prompt"));
    assert_eq!(lines.next().map(str::trim), Some("0"));
}

#[test]
fn a_process_the_agent_leaves_behind_is_stopped_when_the_agent_exits() {
    let script = format!("sleep {SLEEP} >/dev/null 2>&1 & echo $!");
    let out = ask(&example(&["sh", "-c", &script]), "", 5.0).unwrap();
    assert!(gone(out.trim().parse().unwrap()));
}

#[test]
fn an_agent_that_cannot_start_is_refused_naming_its_variable_and_one_that_can_runs() {
    let refused = ask(&example(&["/nonexistent/agent"]), "", 5.0).unwrap_err();
    assert_eq!(refused.code, "example::agent::not_started");
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
    for value in ["claude -p", "[]", "[\"\"]", "[\"claude\", 1]"] {
        assert_eq!(
            named(Some(value)).unwrap_err().code,
            "example::agent::invalid",
            "{value}"
        );
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
    let text = r#"{"findings": [1]} then {"findings": [2], "x": {"y": 1}} and {"other": 3}"#;
    assert_eq!(
        last_object(text, "findings").unwrap()["findings"],
        json!([2])
    );
    assert_eq!(last_object("no object here", "findings"), None);
}
