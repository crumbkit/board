use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;

use board_ops::{Board, Expect, Timings};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::super::budgets::{BUDGETS, Budget};
use super::super::check::{self, Asker, Grace, History, State};
use super::super::digest;
use super::super::grace::ChangedLines;
use super::super::import::{self, Gh, Imported};
use super::super::ready::{self, Past, Thresholds};
use board_model::LogicalPath;

const ROOT: &str = "/boards/personal";
const HOUR: u64 = 3_600_000;
/// A time well after the epoch, on an hour.
const T0: u64 = 1_795_000_000 * 1000 - (1_795_000_000 * 1000) % HOUR;
/// When every test run record started.
const STARTED: &str = "2026-11-23T12:00:00Z";
/// The PATH a test's git and stub gh run with.
const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

/// One line of the log: a top-level run of `command` on the personal
/// board at `at_ms`, taking `ms`, at `load` on 4 cores.
fn line(at_ms: u64, command: &str, ms: u64, load: f64) -> String {
    let (command, capability) = if command.contains('.') {
        ("call", Some(command))
    } else {
        (command, None)
    };
    json!({"$type": "board/timing/v1", "at_ms": at_ms, "command": command,
        "capability": capability, "depth": 0, "root": ROOT, "total_us": ms * 1000,
        "load1": load, "cpus": 4, "killed": false})
    .to_string()
        + "\n"
}

fn log(lines: &[String]) -> Vec<u8> {
    lines.concat().into_bytes()
}

/// Runs of `command` taking each of `ms`, a second apart from `from`.
fn runs(from: u64, command: &str, ms: &[u64], load: f64) -> Vec<String> {
    (ms.iter().enumerate())
        .map(|(index, ms)| line(from + 1000 * index as u64, command, *ms, load))
        .collect()
}

fn history(timings: &Timings, command: &str) -> History {
    let read = check::read(timings, ROOT, T0 + 10 * HOUR);
    read.histories.get(command).cloned().unwrap_or_default()
}

const SLOW: Budget = Budget {
    command: "get",
    ms: 1,
    median_ms: None,
    session_start: false,
};

#[test]
fn the_p50_and_p90_of_the_last_twenty_calm_runs_judge_a_command_and_fewer_than_five_leave_it_unjudged()
 {
    // Five runs on a machine loaded past its cores are left out.
    let mut lines = runs(T0, "get", &[900, 900, 900, 900, 900], 9.0);
    lines.extend(runs(
        T0 + 100_000,
        "get",
        &(1..=25).collect::<Vec<_>>(),
        1.0,
    ));
    let timings = Timings {
        current: log(&lines),
        ..Timings::default()
    };
    let read = history(&timings, "get");
    assert_eq!(read.judged, 25);
    assert_eq!(
        read.judged_us,
        (6..=25).map(|ms| ms * 1000).collect::<Vec<_>>()
    );
    let judged = check::judge(SLOW, Some(&read));
    assert_eq!((judged.p50_us, judged.p90_us), (Some(15_000), Some(23_000)));
    assert_eq!(judged.state, State::Thrice);
    let within = Budget { ms: 30, ..SLOW };
    assert_eq!(check::judge(within, Some(&read)).state, State::Within);
    let over = Budget { ms: 10, ..SLOW };
    assert_eq!(check::judge(over, Some(&read)).state, State::Over);

    let four = Timings {
        current: log(&runs(T0, "get", &[50, 50, 50, 50], 1.0)),
        ..Timings::default()
    };
    assert_eq!(
        check::judge(SLOW, Some(&history(&four, "get"))).state,
        State::Unjudged
    );
    let five = Timings {
        current: log(&runs(T0, "get", &[50, 50, 50, 50, 50], 1.0)),
        ..Timings::default()
    };
    assert_eq!(
        check::judge(SLOW, Some(&history(&five, "get"))).state,
        State::Thrice
    );
}

/// A history of 20 runs that each took `ms`.
fn steady(ms: u64) -> History {
    History {
        judged_us: vec![ms * 1000; 20],
        judged: 20,
        killed: 0,
    }
}

#[test]
fn a_median_over_its_budget_makes_a_command_over_while_its_p90_is_within() {
    let next = Budget {
        command: "changes.next",
        ms: 10_000,
        median_ms: Some(2_000),
        session_start: false,
    };
    let judged = check::judge(next, Some(&steady(3_000)));
    assert_eq!(judged.state, State::Over);
    let note = check::run_record(&[judged], &BTreeSet::new(), STARTED, "0.0.0", &host());
    assert_eq!(
        note["results"][0]["note"],
        "p50 3000 ms is over the 2000 ms median budget"
    );
    assert_eq!(note["results"][0]["measures"]["median_budget_ms"], 2_000);
    assert_eq!(
        check::judge(next, Some(&steady(7_000))).state,
        State::Thrice
    );
    assert_eq!(
        check::judge(next, Some(&steady(1_500))).state,
        State::Within
    );
    assert_eq!(
        check::judge(
            Budget {
                median_ms: None,
                ..next
            },
            Some(&steady(3_000))
        )
        .state,
        State::Within
    );
}

#[test]
fn runs_on_another_board_nested_runs_and_lines_that_do_not_parse_are_left_out() {
    let mut lines = runs(T0, "get", &[5; 5], 1.0);
    lines.push(line(T0 + 9000, "get", 500, 1.0).replace(ROOT, "/boards/other"));
    lines.push(line(T0 + 9500, "get", 500, 1.0).replace("\"depth\":0", "\"depth\":1"));
    lines.push("{\"at_ms\": 3, \"comm\n".to_owned());
    let timings = Timings {
        current: log(&lines),
        ..Timings::default()
    };
    let read = check::read(&timings, ROOT, T0);
    assert_eq!(read.histories["get"].judged, 5);
    assert_eq!(read.skipped, 1);
}

#[test]
fn a_killed_session_start_run_marks_its_command_far_too_slow_and_a_killed_run_elsewhere_counts_only()
 {
    let mut lines = runs(T0, "rules.check", &[5; 5], 1.0);
    lines.push(
        line(T0 + 9000, "rules.check", 5000, 1.0).replace("\"killed\":false", "\"killed\":true"),
    );
    lines.extend(runs(T0, "rules.for-path", &[5; 5], 1.0));
    lines.push(
        line(T0 + 9000, "rules.for-path", 5000, 1.0).replace("\"killed\":false", "\"killed\":true"),
    );
    let timings = Timings {
        current: log(&lines),
        ..Timings::default()
    };
    let read = check::read(&timings, ROOT, T0 + HOUR);
    let judged = check::judge_all(&read, &BUDGETS);
    let of = |command: &str| judged.iter().find(|j| j.budget.command == command).unwrap();
    assert_eq!(
        (of("rules.check").state, of("rules.check").killed),
        (State::Thrice, 1)
    );
    assert_eq!(
        (of("rules.for-path").state, of("rules.for-path").killed),
        (State::Within, 1)
    );
    let all: BTreeSet<String> = BUDGETS.iter().map(|b| b.command.to_owned()).collect();
    assert_eq!(check::counts(&judged, &all), (1, 2));
    // A silent command's results count apart, as the zero rule reads only
    // the live ones.
    assert_eq!(check::counts(&judged, &BTreeSet::new()), (0, 0));
}

#[test]
fn after_rotation_the_check_sees_the_same_runs_through_the_digest() {
    let older = log(&runs(T0, "get", &(1..=12).collect::<Vec<_>>(), 1.0));
    let newer = log(&runs(
        T0 + 2 * HOUR,
        "get",
        &(13..=18).collect::<Vec<_>>(),
        1.0,
    ));
    let newest = log(&runs(
        T0 + 4 * HOUR,
        "get",
        &(19..=25).collect::<Vec<_>>(),
        1.0,
    ));
    let before = Timings {
        digest: Vec::new(),
        previous: [older.clone(), newer.clone()].concat(),
        current: newest.clone(),
    };
    // The log of older and newer runs rotated twice: older's runs now
    // live only in the digest.
    let digest = digest::summarise(&older, b"");
    let digest = digest::summarise(&newer, &digest);
    let after = Timings {
        digest: digest.clone(),
        previous: newer.clone(),
        current: newest.clone(),
    };
    assert_eq!(history(&after, "get"), history(&before, "get"));
    assert_eq!(history(&after, "get").judged, 25);
    // A rotation that ran again after a crash adds nothing, and a line
    // cut short is dropped.
    let mut torn = digest.clone();
    torn.extend(b"{\"$type\":\"board/timing-digest/v1\",\"fr");
    assert_eq!(digest::summarise(&newer, &torn), digest);
}

/// The digest after each log in turn rotates, within `window`.
fn rotated(logs: &[Vec<u8>], window: &digest::Window) -> Vec<digest::Hour> {
    let mut bytes = Vec::new();
    for log in logs {
        bytes = digest::summarise_within(log, &bytes, window);
    }
    digest::hours(&bytes).0
}

/// One log per hour from T0, each with two calm runs of `get`.
fn hourly(hours: u64) -> Vec<Vec<u8>> {
    (0..hours)
        .map(|hour| log(&runs(T0 + hour * HOUR, "get", &[5, 5], 1.0)))
        .collect()
}

fn starts(hours: &[digest::Hour]) -> Vec<u64> {
    hours
        .iter()
        .map(|hour| (hour.start_ms - T0) / HOUR)
        .collect()
}

#[test]
fn a_digest_keeps_each_commands_hours_that_hold_its_last_judged_times_and_the_last_day_and_drops_older_ones()
 {
    let window = digest::Window {
        judged: 3,
        recent_ms: 0,
        hours: 100,
    };
    // Hour 5 holds 2 judged times and hour 4 two more, which reach 3.
    assert_eq!(starts(&rotated(&hourly(6), &window)), [4, 5]);
    let day = digest::Window {
        recent_ms: 3 * HOUR,
        ..window
    };
    assert_eq!(starts(&rotated(&hourly(6), &day)), [2, 3, 4, 5]);
    // A command keeps its own hours, whatever another command ran since.
    let mut logs = vec![log(&runs(T0, "put", &[5, 5, 5], 1.0))];
    logs.extend(hourly(6).into_iter().skip(1));
    let kept = rotated(&logs, &window);
    let put: Vec<_> = kept.iter().filter(|hour| hour.command == "put").collect();
    assert_eq!(put.len(), 1);
}

#[test]
fn a_digest_past_its_most_hours_drops_the_oldest_hours_first() {
    let window = digest::Window {
        judged: 1_000,
        recent_ms: 1_000 * HOUR,
        hours: 3,
    };
    assert_eq!(starts(&rotated(&hourly(6), &window)), [3, 4, 5]);
    // The digest the binary keeps holds a day and the times the check reads.
    assert_eq!(
        (
            digest::WINDOW.judged,
            digest::WINDOW.recent_ms,
            digest::WINDOW.hours
        ),
        (digest::KEPT, 24 * HOUR, 2_000)
    );
}

/// `ms` as budgets.md writes it, with a thousands comma.
fn shown(ms: u64) -> String {
    let ms = ms.to_string();
    match ms.len() {
        4.. => format!("{},{}", &ms[..ms.len() - 3], &ms[ms.len() - 3..]),
        _ => ms,
    }
}

#[test]
fn the_budgets_hold_the_owners_figures_and_budgets_md_shows_the_same_table() {
    let of = |command: &str| *BUDGETS.iter().find(|b| b.command == command).unwrap();
    for command in ["query", "plans.waiting", "orders.pinned", "rules.check"] {
        assert_eq!(of(command).ms, 100, "{command}");
    }
    let halved = [
        ("get", 25),
        ("put", 150),
        ("plans.open", 250),
        ("rules.for-path", 500),
        ("tasks.available", 1_500),
        ("changes.open", 1_500),
        ("changes.next", 10_000),
    ];
    for (command, ms) in halved {
        assert_eq!(of(command).ms, ms, "{command}");
    }
    // The four the owner added on 26 November 2026.
    assert_eq!((of("ls").ms, of("ls").session_start), (500, true));
    for command in ["changes.next", "changes.advance", "changes.brief"] {
        assert_eq!(
            (of(command).ms, of(command).median_ms),
            (10_000, Some(2_000)),
            "{command}"
        );
    }
    let docs = include_str!("../../../../../docs/architecture/performance/budgets.md");
    let rows: Vec<String> = (docs.lines())
        .skip_while(|line| *line != "| Command | Budget (p90) | Budget (median) | Session start |")
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(str::to_owned)
        .collect();
    let wanted: Vec<String> = (BUDGETS.iter())
        .map(|budget| {
            let median = budget
                .median_ms
                .map_or_else(|| "-".to_owned(), |ms| format!("{} ms", shown(ms)));
            let start = if budget.session_start { "yes" } else { "no" };
            format!(
                "| `{}` | {} ms | {median} | {start} |",
                budget.command,
                shown(budget.ms)
            )
        })
        .collect();
    assert_eq!(rows, wanted);
}

/// A personal board with the schemas the check writes.
fn personal() -> Scenario {
    let scenario = Scenario::board();
    for (path, schema) in [
        (
            "runs/run/v1",
            include_str!("../../../../board-plugin-runs/types/runs/run/v1.schema.json"),
        ),
        (
            "runs/suite/v1",
            include_str!("../../../../board-plugin-runs/types/runs/suite/v1.schema.json"),
        ),
        (
            "tasks/task/v3",
            include_str!("../../../../board-plugin-tasks/types/tasks/task/v3.schema.json"),
        ),
        (
            "plans/question/v1",
            include_str!("../../../../board-plugin-plans/types/plans/question/v1.schema.json"),
        ),
        (
            "plans/answer/v1",
            include_str!("../../../../board-plugin-plans/types/plans/answer/v1.schema.json"),
        ),
        (
            "rules/rule/v1",
            include_str!("../../../../board-plugin-rules/types/rules/rule/v1.schema.json"),
        ),
    ] {
        scenario.write(
            &format!(".board/types/{path}.schema.json"),
            Item::File(schema),
        );
    }
    scenario.identified();
    scenario
}

fn open(scenario: &Scenario) -> Board {
    Board::open(scenario.workdir(), None).unwrap()
}

fn open_at(root: &std::path::Path) -> Board {
    Board::open(root, None).unwrap()
}

/// A grace of 100 lines, with `changed` lines since every task opened.
struct Lines(Option<u64>);

impl Grace for Lines {
    fn allowed(&self) -> u64 {
        100
    }

    fn changed_since(&self, _: jiff::Timestamp) -> Option<u64> {
        self.0
    }
}

fn judged(command: &str, p90_ms: u64) -> check::Judged {
    let budget = *BUDGETS.iter().find(|b| b.command == command).unwrap();
    check::judge(budget, Some(&steady(p90_ms)))
}

fn host() -> check::Host {
    check::Host {
        name: Some("studio".to_owned()),
        os: "macos",
        arch: "aarch64",
        cpus: 14,
    }
}

/// Every command live.
fn all_live() -> BTreeSet<String> {
    BUDGETS.iter().map(|b| b.command.to_owned()).collect()
}

/// The local run `name`, which started at [`STARTED`], written to `board`.
fn run_path(board: &Board, name: &str) -> LogicalPath {
    let path: LogicalPath = format!("/suites/board-speed-local/runs/{name}")
        .parse()
        .unwrap();
    if board.get(&path).is_err() {
        let run = check::run_record(&[], &BTreeSet::new(), STARTED, "0.0.0", &host());
        board
            .put(&path, Expect::Absent, run.to_string().as_bytes())
            .unwrap();
    }
    path
}

fn session() -> Asker<'static> {
    Asker {
        at: STARTED,
        session: Some("local_0a1b"),
    }
}

fn text(board: &Board, path: &str) -> Option<String> {
    let got = board.get(&path.parse().unwrap()).ok()?;
    Some(String::from_utf8(got.bytes).unwrap())
}

#[test]
fn a_first_over_budget_result_opens_a_task_and_files_no_question_and_one_within_opens_none() {
    let scenario = personal();
    let board = open(&scenario);
    let over = [judged("plans.open", 300), judged("get", 5)];
    let run = run_path(&board, "a");
    let alerted = check::alert(
        &board,
        &over,
        &all_live(),
        &run,
        &session(),
        &Lines(Some(1_000)),
    )
    .unwrap();
    assert_eq!(alerted.tasks, ["/tasks/board-speed-plans-open.md"]);
    assert_eq!(alerted.questions, Vec::<String>::new());
    let task = text(&board, "/tasks/board-speed-plans-open.md").unwrap();
    assert!(
        task.contains("title: \"Board's plans.open is over its 250 ms budget\""),
        "{task}"
    );
    assert!(task.contains("/suites/board-speed-local/runs/a"), "{task}");
    assert_eq!(text(&board, "/tasks/board-speed-get.md"), None);
}

#[test]
fn a_silent_command_over_budget_opens_no_task() {
    let scenario = personal();
    let board = open(&scenario);
    let over = [judged("plans.open", 300), judged("get", 500)];
    let live: BTreeSet<String> = ["get".to_owned()].into();
    let run = run_path(&board, "a");
    let alerted = check::alert(&board, &over, &live, &run, &session(), &Lines(None)).unwrap();
    assert_eq!(alerted.tasks, ["/tasks/board-speed-get.md"]);
    assert_eq!(text(&board, "/tasks/board-speed-plans-open.md"), None);
}

#[test]
fn a_command_over_three_times_its_budget_gets_a_task_whose_title_says_so_and_an_open_task_is_retitled()
 {
    let scenario = personal();
    let board = open(&scenario);
    let path = "/tasks/board-speed-plans-open.md";
    let alert = |p90: u64, run: &str| {
        let run = run_path(&board, run);
        check::alert(
            &board,
            &[judged("plans.open", p90)],
            &all_live(),
            &run,
            &session(),
            &Lines(None),
        )
        .unwrap()
    };
    alert(300, "a");
    let alerted = alert(900, "b");
    assert_eq!(alerted.tasks, [path]);
    let task = text(&board, path).unwrap();
    assert!(
        task.contains("far too slow, so the lead names it to the owner: p90 900 ms is over three times the 250 ms budget"),
        "{task}"
    );
    assert!(
        task.contains("/suites/board-speed-local/runs/a"),
        "the task keeps the run that opened it: {task}"
    );
    assert_eq!(alert(900, "c").tasks, Vec::<String>::new());
}

#[test]
fn a_command_still_over_after_the_grace_files_one_question_and_one_within_the_grace_files_none() {
    let scenario = personal();
    let board = open(&scenario);
    let over = [judged("plans.open", 300)];
    let alert = |run: &str, changed: u64| {
        let run = run_path(&board, run);
        check::alert(
            &board,
            &over,
            &all_live(),
            &run,
            &session(),
            &Lines(Some(changed)),
        )
        .unwrap()
    };
    alert("a", 1_000);
    assert_eq!(alert("b", 100).questions, Vec::<String>::new());
    assert_eq!(
        alert("c", 101).questions,
        ["/projects/board/questions/board-speed-plans-open"]
    );
    assert_eq!(alert("d", 500).questions, Vec::<String>::new());
    let question: Value = serde_json::from_str(
        &text(&board, "/projects/board/questions/board-speed-plans-open").unwrap(),
    )
    .unwrap();
    assert_eq!(question["status"], "open");
    assert_eq!(question["blocks"], "/tasks/board-speed-plans-open.md");
}

#[test]
fn a_question_names_the_asking_session_and_a_check_without_one_or_with_a_speed_question_open_files_none()
 {
    let scenario = personal();
    let board = open(&scenario);
    let over = [judged("plans.open", 300)];
    let run = run_path(&board, "a");
    check::alert(&board, &over, &all_live(), &run, &session(), &Lines(None)).unwrap();
    let anonymous = Asker {
        at: STARTED,
        session: None,
    };
    let none = check::alert(
        &board,
        &over,
        &all_live(),
        &run,
        &anonymous,
        &Lines(Some(500)),
    );
    assert_eq!(none.unwrap().questions, Vec::<String>::new());

    // Another command's speed question is open, so this one waits.
    let other: LogicalPath = "/projects/board/questions/board-speed-get".parse().unwrap();
    let open_question = json!({"$type": "plans/question/v1", "question": "Should get's budget change?",
        "by": {"session": "local_ff"}, "asked": STARTED, "status": "open"});
    board
        .put(&other, Expect::Absent, open_question.to_string().as_bytes())
        .unwrap();
    let waiting = check::alert(
        &board,
        &over,
        &all_live(),
        &run,
        &session(),
        &Lines(Some(500)),
    );
    assert_eq!(waiting.unwrap().questions, Vec::<String>::new());

    // The owner answers it, so it is no longer open.
    let answer: LogicalPath = "/projects/board/answers/board-speed-get".parse().unwrap();
    let answered = json!({"$type": "plans/answer/v1", "question": other.as_str(),
        "choice": "a", "words": "Keep it", "by": {"person": "owner"}, "answered": STARTED});
    board
        .put(&answer, Expect::Absent, answered.to_string().as_bytes())
        .unwrap();
    let asked = check::alert(
        &board,
        &over,
        &all_live(),
        &run,
        &session(),
        &Lines(Some(500)),
    );
    assert_eq!(
        asked.unwrap().questions,
        ["/projects/board/questions/board-speed-plans-open"]
    );
    let question: Value = serde_json::from_str(
        &text(&board, "/projects/board/questions/board-speed-plans-open").unwrap(),
    )
    .unwrap();
    assert_eq!(
        question["by"],
        json!({"program": "board", "session": "local_0a1b"})
    );
}

/// The environment a test's git or gh runs with: no user configuration.
fn environment(home: &Path) -> Vec<(String, OsString)> {
    vec![
        ("PATH".to_owned(), PATH.into()),
        ("HOME".to_owned(), home.into()),
        ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".into()),
    ]
}

fn epoch(at: &str) -> i64 {
    at.parse::<jiff::Timestamp>().unwrap().as_second()
}

/// `count` numbered lines.
fn numbered(count: usize) -> String {
    (0..count).fold(String::new(), |mut text, line| {
        let _ = writeln!(text, "line {line}");
        text
    })
}

#[test]
fn an_allowance_of_ten_files_one_question_at_eleven_changed_lines_and_none_at_ten() {
    let repository = Scenario::new();
    let since = epoch(STARTED);
    let old = numbered(100);
    let before = repository.commit_at(&[("a.txt", &old)], &[], since - 3_600, "before");
    let ten = numbered(10);
    let after = repository.commit_at(
        &[("a.txt", &old), ("b.txt", &ten)],
        &[before],
        since + 3_600,
        "ten lines",
    );
    let home = tempfile::tempdir().unwrap();
    let lines = ChangedLines {
        repository: repository.workdir().to_path_buf(),
        follow: "HEAD".to_owned(),
        allowed: 10,
        environment: environment(home.path()),
    };
    let opened: jiff::Timestamp = STARTED.parse().unwrap();
    assert_eq!(lines.changed_since(opened), Some(10));

    let scenario = personal();
    let board = open(&scenario);
    let over = [judged("plans.open", 300)];
    let run = run_path(&board, "a");
    check::alert(&board, &over, &all_live(), &run, &session(), &lines).unwrap();
    let at_ten = check::alert(&board, &over, &all_live(), &run, &session(), &lines).unwrap();
    assert_eq!(at_ten.questions, Vec::<String>::new());

    repository.commit_at(
        &[("a.txt", &old), ("b.txt", &numbered(11))],
        &[after],
        since + 7_200,
        "one more line",
    );
    assert_eq!(lines.changed_since(opened), Some(11));
    let at_eleven = check::alert(&board, &over, &all_live(), &run, &session(), &lines).unwrap();
    assert_eq!(
        at_eleven.questions,
        ["/projects/board/questions/board-speed-plans-open"]
    );

    let missing = ChangedLines {
        repository: home.path().join("no-repository"),
        ..lines
    };
    assert_eq!(missing.changed_since(opened), None);
}

#[test]
fn two_checks_at_once_open_one_task() {
    let scenario = personal();
    let over = [judged("plans.open", 300)];
    let runs: Vec<LogicalPath> = (0..2)
        .map(|index| run_path(&open(&scenario), &format!("r{index}")))
        .collect();
    let opened: Vec<_> = std::thread::scope(|scope| {
        let threads: Vec<_> = (runs.iter())
            .map(|run| {
                let root = scenario.workdir().to_path_buf();
                let over = &over;
                scope.spawn(move || {
                    let board = open_at(&root);
                    check::alert(&board, over, &all_live(), run, &session(), &Lines(None))
                        .unwrap()
                        .tasks
                })
            })
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect()
    });
    assert_eq!(opened, ["/tasks/board-speed-plans-open.md"]);
}

#[test]
fn the_run_record_and_the_suite_pass_their_schemas_and_name_each_commands_mode() {
    let scenario = personal();
    let board = open(&scenario);
    let suite = check::local_suite();
    assert_eq!(suite["kind"], "benchmark");
    let judged: Vec<_> = (BUDGETS.iter())
        .map(|budget| check::judge(*budget, None))
        .chain([judged("plans.open", 300), judged("get", 5)])
        .collect();
    let live: BTreeSet<String> = ["plans.open".to_owned()].into();
    let run = check::run_record(&judged, &live, STARTED, "0.0.0+abc", &host());
    assert_eq!(
        run["measures"],
        json!({"over_budget": 1, "killed": 0, "silent_over_budget": 0, "silent_killed": 0})
    );
    assert_eq!(run["dirty"], false);
    assert_eq!(
        run["setup"],
        "board timing check --record on the machine studio"
    );
    let modes: Vec<_> = (run["results"].as_array().unwrap().iter())
        .map(|result| result["measures"]["mode"].as_str().unwrap())
        .collect();
    // plans.open is judged twice here, once unjudged and once over.
    assert_eq!(modes.iter().filter(|mode| **mode == "live").count(), 2);
    assert_eq!(ready::live_in(&run), live);
    let dirty = check::run_record(&judged, &live, STARTED, "0.0.0+abc.dirty.1f", &host());
    assert_eq!(dirty["dirty"], true);
    for (path, record) in [
        ("/suites/board-speed-local", suite),
        ("/suites/board-speed-ci", import::ci_suite()),
        ("/suites/board-speed-local/runs/a", run),
    ] {
        let path = path.parse().unwrap();
        board
            .put(&path, Expect::Absent, record.to_string().as_bytes())
            .unwrap();
    }
}

/// A run of the CI suite with a result of `outcome` for each command.
fn ci_run(results: &[(&str, &str)]) -> Value {
    let results: Vec<Value> = (results.iter())
        .map(|(case, outcome)| {
            let mut result = json!({"case": case, "outcome": outcome, "measures": {"p90_ms": 1.0}});
            if *outcome != "pass" {
                result["note"] = json!("over");
            }
            result
        })
        .collect();
    json!({"$type": "runs/run/v1", "suite": import::CI_SUITE, "started": STARTED,
        "scope": "full", "dirty": false, "results": results, "measures": {"over_budget_any": 0}})
}

/// The live commands of a check run after `local` and `ci`, then `local`
/// with that check's run record added.
fn checked(
    local: &mut Vec<Value>,
    ci: &[Value],
    judged: &[check::Judged],
    thresholds: &Thresholds,
) -> BTreeSet<String> {
    let past = Past::of(local, ci);
    let live = ready::live(&past, judged, thresholds);
    local.push(check::run_record(judged, &live, STARTED, "0.0.0", &host()));
    live
}

#[test]
fn with_thresholds_of_three_local_and_two_ci_runs_a_command_goes_live_at_the_third_run_and_stays_live()
 {
    let thresholds = Thresholds { local: 3, ci: 2 };
    let ci = [ci_run(&[("get", "pass")]), ci_run(&[("get", "pass")])];
    let mut local = Vec::new();
    for check in 1..=3 {
        let live = checked(&mut local, &ci, &[judged("get", 5)], &thresholds);
        assert_eq!(live.contains("get"), check == 3, "check {check}");
    }
    // Live stays live, whatever the data and with no CI run read.
    let live = checked(&mut local, &[], &[judged("get", 500)], &thresholds);
    assert!(live.contains("get"));
    let live = checked(&mut local, &[], &[check::judge(SLOW, None)], &thresholds);
    assert!(live.contains("get"));
}

#[test]
fn a_command_waits_for_enough_ci_runs_once_ci_has_one_and_one_that_flaps_stays_silent() {
    let thresholds = Thresholds { local: 3, ci: 2 };
    let one = [ci_run(&[("get", "pass")])];
    let mut local = Vec::new();
    for _ in 1..=4 {
        let live = checked(&mut local, &one, &[judged("get", 5)], &thresholds);
        assert!(live.is_empty());
    }
    // With no CI result for the command, local runs alone decide.
    let mut alone = Vec::new();
    for check in 1..=3 {
        let live = checked(&mut alone, &[], &[judged("get", 5)], &thresholds);
        assert_eq!(live.contains("get"), check == 3, "check {check}");
    }
    // Over, within, over: one of its two over-budget results cleared at the
    // next result, which is more than 1 in 10.
    let mut flapping = Vec::new();
    for p90 in [500, 5, 500, 500] {
        let live = checked(&mut flapping, &[], &[judged("get", p90)], &thresholds);
        assert!(live.is_empty(), "{p90}");
    }
    // An unjudged result neither counts nor clears.
    let mut unjudged = Vec::new();
    let empty = check::judge(SLOW, None);
    for judged in [
        empty.clone(),
        empty.clone(),
        judged("get", 5),
        judged("get", 5),
    ] {
        let live = checked(&mut unjudged, &[], &[judged], &thresholds);
        assert!(live.is_empty());
    }
    assert!(checked(&mut unjudged, &[], &[judged("get", 5)], &thresholds).contains("get"));
}

#[test]
fn the_first_live_command_writes_the_speed_rules_once_and_they_pass_their_schema() {
    let scenario = personal();
    let board = open(&scenario);
    let written = ready::rules(&board).unwrap();
    assert_eq!(
        written,
        [
            "/rules/board/speed-budgets",
            "/rules/board/speed-budgets-ci"
        ]
    );
    let rule: Value =
        serde_json::from_str(&text(&board, "/rules/board/speed-budgets").unwrap()).unwrap();
    assert_eq!(
        rule["checker"],
        json!({"check": "zero", "suite": "/suites/board-speed-local", "measures": ["over_budget", "killed"]})
    );
    assert_eq!(ready::rules(&board).unwrap(), Vec::<String>::new());
    let checked = board.check(None, None).unwrap();
    let invalid: Vec<_> = (checked.entries.iter())
        .filter(|entry| entry.status != board_tree::Status::Ok)
        .filter(|entry| entry.kind == board_model::Kind::Record)
        .map(|entry| format!("{:?} {:?}", entry.logical, entry.status))
        .collect();
    assert_eq!(invalid, Vec::<String>::new());
}

/// A gh on PATH that lists one run, 42, of the timing workflow and
/// downloads `run` as its artifact, noting each download in `downloads`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test writes a stub gh and the artifact it downloads"
)]
fn stub_gh(directory: &Path, run: &Value) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(directory.join("artifact.json"), run.to_string()).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n\
         'run list') echo '[{{\"databaseId\": 42}}]' ;;\n\
         'run download') while [ $# -gt 0 ]; do [ \"$1\" = --dir ] && dir=$2; shift; done\n\
           cp '{0}/artifact.json' \"$dir/run.json\"; echo download >> '{0}/downloads' ;;\n\
         *) exit 2 ;;\nesac\n",
        directory.display()
    );
    let gh = directory.join("gh");
    std::fs::write(&gh, script).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    gh
}

#[test]
fn a_stubbed_gh_run_is_imported_once_and_a_missing_gh_warns_and_writes_nothing() {
    let scenario = personal();
    let board = open(&scenario);
    let stub = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let run = ci_run(&[("get", "fail"), ("put", "pass")]);
    let gh = Gh {
        program: stub_gh(stub.path(), &run).display().to_string(),
        environment: environment(stub.path()),
        directory: downloads.path().to_path_buf(),
    };
    let live: BTreeSet<String> = ["get".to_owned()].into();

    let missing = Gh {
        program: stub.path().join("no-gh").display().to_string(),
        ..gh.clone()
    };
    let warned = import::import(&board, &missing, &live);
    assert!(warned.is_err(), "{warned:?}");
    assert_eq!(text(&board, "/suites/board-speed-ci"), None);

    let imported = import::import(&board, &gh, &live).unwrap();
    let path = "/suites/board-speed-ci/runs/42";
    assert_eq!(imported, Imported::Wrote(path.parse().unwrap()));
    let held: Value = serde_json::from_str(&text(&board, path).unwrap()).unwrap();
    assert_eq!(held["suite"], import::CI_SUITE);
    assert_eq!(held["measures"]["over_budget"], 1);
    assert_eq!(held["measures"]["killed"], 0);
    assert!(text(&board, import::CI_SUITE).is_some());

    assert_eq!(import::import(&board, &gh, &live).unwrap(), Imported::Held);
    let downloaded = std::fs::read_to_string(stub.path().join("downloads")).unwrap();
    assert_eq!(downloaded.lines().count(), 1);

    let bad = Gh {
        program: stub_gh(stub.path(), &json!({"$type": "runs/run/v1"}))
            .display()
            .to_string(),
        ..gh
    };
    let scenario = personal();
    let other = open(&scenario);
    assert!(import::import(&other, &bad, &live).is_err());
    assert_eq!(text(&other, path), None);
}
