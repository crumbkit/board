use std::time::Instant;

use board_model::{Code, Fields, Refusal};

use super::*;
use crate::render::{self, Printer};

fn ran(command: &str) -> Ran {
    Ran {
        command: Some(command.to_owned()),
        root: Some(PathBuf::from("/boards/one")),
        ..Ran::default()
    }
}

#[test]
fn a_record_is_written_as_these_exact_bytes_but_for_its_times() {
    let mut ran = ran("check");
    ran.trace.add("snapshot", Instant::now());
    ran.trace.add("snapshot", Instant::now());
    ran.trace.count("entries", 3);
    ran.trace.count("parsed", 0);
    let output = Output {
        code: 1,
        error: Some("board::path::not_found".to_owned()),
        ..Output::default()
    };
    let machine = Machine {
        load1: Some(2.456),
        cpus: Some(14),
    };
    let mut record = record(Instant::now(), &ran, &output, 1, machine, false).unwrap();
    for (pointer, time) in [
        ("/at_ms", 1_791_250_685_132_u64),
        ("/total_us", 1280),
        ("/phases/0/at_us", 149),
        ("/phases/0/us", 1125),
    ] {
        *record.pointer_mut(pointer).unwrap() = time.into();
    }
    *record.pointer_mut("/version").unwrap() = "0.0.0".into();
    let golden = concat!(
        r#"{"$type":"board/timing/v1","at_ms":1791250685132,"capability":null,"#,
        r#""command":"check","cpus":14,"depth":1,"#,
        r#""error":"board::path::not_found","exit":1,"killed":false,"load1":2.46,"#,
        r#""phases":[{"at_us":149,"entered":2,"name":"snapshot","us":1125}],"prefix":null,"#,
        r#""root":"/boards/one","total_us":1280,"version":"0.0.0","#,
        r#""work":{"entries":3,"parsed":0}}"#,
    );
    assert_eq!(record.to_string(), golden);
}

#[test]
fn a_run_whose_command_line_was_not_parsed_has_no_record_and_a_parsed_one_has_one() {
    let output = Output::default();
    assert_eq!(
        record(
            Instant::now(),
            &Ran::default(),
            &output,
            0,
            Machine::default(),
            false
        ),
        None
    );
    assert!(
        record(
            Instant::now(),
            &ran("get"),
            &output,
            0,
            Machine::default(),
            false
        )
        .is_some()
    );
}

#[test]
fn phases_are_recorded_in_the_order_they_started_whatever_order_they_ended() {
    let started = Instant::now();
    let mut ran = ran("check");
    let outer = started + Duration::from_micros(10);
    let inner = started + Duration::from_micros(20);
    // An inner phase ends, and so is traced, before the phase around it.
    ran.trace.add("index.refresh", inner);
    ran.trace.add("snapshot", outer);
    let record = record(
        started,
        &ran,
        &Output::default(),
        0,
        Machine::default(),
        false,
    )
    .unwrap();
    let phases = &record["phases"];
    assert_eq!(
        (&phases[0]["name"], &phases[1]["name"]),
        (&json!("snapshot"), &json!("index.refresh"))
    );
    assert_eq!(
        (&phases[0]["at_us"], &phases[1]["at_us"]),
        (&json!(10), &json!(20))
    );
}

#[test]
fn of_two_phases_that_started_in_the_same_tick_the_longer_one_around_the_other_comes_first() {
    let started = Instant::now();
    let mut ran = ran("check");
    for (name, took) in [("index.refresh", 5), ("snapshot", 9)] {
        ran.trace.phases.push(board_ops::Phase {
            name,
            started,
            took: Duration::from_micros(took),
            entered: 1,
        });
    }
    let record = record(
        started,
        &ran,
        &Output::default(),
        0,
        Machine::default(),
        false,
    )
    .unwrap();
    let phases = &record["phases"];
    assert_eq!(
        (&phases[0]["name"], &phases[1]["name"]),
        (&json!("snapshot"), &json!("index.refresh"))
    );
}

#[test]
fn a_duration_is_recorded_in_whole_microseconds() {
    assert_eq!(micros(Duration::from_nanos(2_500_999)), 2_500);
    assert_eq!(micros(Duration::ZERO), 0);
}

#[test]
fn a_refusal_names_its_code_for_the_record_in_text_and_json() {
    let boards = Refusal::new(Code::PutInvalid, "refused", &Fields::default());
    let plugins = Refusal::plugin(
        board_model::PluginCode::parse(&"tasks".parse().unwrap(), "tasks::date::bad".to_owned())
            .unwrap(),
        "bad date".to_owned(),
        serde_json::Map::new(),
    );
    for json in [false, true] {
        let printer = || Printer::of(json, 1 << 20, &[]);
        let refused = render::refused(printer(), boards.clone());
        assert_eq!(refused.error.as_deref(), Some("board::put::invalid"));
        let refused = render::refused(printer(), plugins.clone());
        assert_eq!(refused.error.as_deref(), Some("tasks::date::bad"));
    }
}

#[test]
fn a_run_a_signal_ends_is_recorded_as_killed_with_no_exit_code() {
    let record = record(
        Instant::now(),
        &ran("call"),
        &Output::default(),
        0,
        Machine::default(),
        true,
    )
    .unwrap();
    assert_eq!(
        (&record["killed"], &record["exit"]),
        (&json!(true), &Value::Null)
    );
}

mod speed {
    use board_ops::{Board, Timings};
    use board_scenario::{Item, Scenario};
    use serde_json::{Value, json};

    use super::super::budgets::{BUDGETS, Budget};
    use super::super::check::{self, Grace, History, State};
    use super::super::digest;
    use board_model::LogicalPath;

    const ROOT: &str = "/boards/personal";
    const HOUR: u64 = 3_600_000;
    /// A time well after the epoch, on an hour.
    const T0: u64 = 1_795_000_000 * 1000 - (1_795_000_000 * 1000) % HOUR;

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
            line(T0 + 9000, "rules.check", 5000, 1.0)
                .replace("\"killed\":false", "\"killed\":true"),
        );
        lines.extend(runs(T0, "rules.for-path", &[5; 5], 1.0));
        lines.push(
            line(T0 + 9000, "rules.for-path", 5000, 1.0)
                .replace("\"killed\":false", "\"killed\":true"),
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
        assert_eq!(check::counts(&judged), (1, 2));
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

    #[test]
    fn the_budgets_hold_the_owners_figures_and_budgets_md_shows_the_same_table() {
        let of = |command: &str| BUDGETS.iter().find(|b| b.command == command).unwrap().ms;
        for command in ["query", "plans.waiting", "attention.pinned", "rules.check"] {
            assert_eq!(of(command), 100, "{command}");
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
            assert_eq!(of(command), ms, "{command}");
        }
        let docs = include_str!("../../../../docs/architecture/performance/budgets.md");
        let rows: Vec<String> = (docs.lines())
            .skip_while(|line| *line != "| Command | Budget (p90) | Session start |")
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(str::to_owned)
            .collect();
        let wanted: Vec<String> = (BUDGETS.iter())
            .map(|budget| {
                let ms = budget.ms.to_string();
                let ms = match ms.len() {
                    4.. => format!("{},{}", &ms[..ms.len() - 3], &ms[ms.len() - 3..]),
                    _ => ms,
                };
                let start = if budget.session_start { "yes" } else { "no" };
                format!("| `{}` | {ms} ms | {start} |", budget.command)
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
                include_str!("../../../board-plugin-runs/types/runs/run/v1.schema.json"),
            ),
            (
                "runs/suite/v1",
                include_str!("../../../board-plugin-runs/types/runs/suite/v1.schema.json"),
            ),
            (
                "tasks/task/v3",
                include_str!("../../../board-plugin-tasks/types/tasks/task/v3.schema.json"),
            ),
            (
                "plans/question/v1",
                include_str!("../../../board-plugin-plans/types/plans/question/v1.schema.json"),
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

        fn changed_since(&self, _: &LogicalPath) -> Option<u64> {
            self.0
        }
    }

    fn judged(command: &str, p90_ms: u64) -> check::Judged {
        let budget = *BUDGETS.iter().find(|b| b.command == command).unwrap();
        let history = History {
            judged_us: vec![p90_ms * 1000; 20],
            judged: 20,
            killed: 0,
        };
        check::judge(budget, Some(&history))
    }

    fn run_path(name: &str) -> LogicalPath {
        format!("/suites/board-speed-local/runs/{name}")
            .parse()
            .unwrap()
    }

    fn asked() -> (&'static str, &'static Value) {
        static BY: std::sync::LazyLock<Value> =
            std::sync::LazyLock::new(|| json!({"program": "claude-code", "session": "local_0a1b"}));
        ("2026-11-23T12:00:00Z", &BY)
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
        let alerted =
            check::alert(&board, &over, &run_path("a"), asked(), &Lines(Some(1_000))).unwrap();
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
    fn a_command_over_three_times_its_budget_gets_a_task_whose_title_says_so_and_an_open_task_is_retitled()
     {
        let scenario = personal();
        let board = open(&scenario);
        let path = "/tasks/board-speed-plans-open.md";
        check::alert(
            &board,
            &[judged("plans.open", 300)],
            &run_path("a"),
            asked(),
            &Lines(None),
        )
        .unwrap();
        let alerted = check::alert(
            &board,
            &[judged("plans.open", 900)],
            &run_path("b"),
            asked(),
            &Lines(None),
        )
        .unwrap();
        assert_eq!(alerted.tasks, [path]);
        let task = text(&board, path).unwrap();
        assert!(task.contains("far too slow, so the lead names it to the owner: p90 900 ms is over three times the 250 ms budget"), "{task}");
        assert!(
            task.contains("/suites/board-speed-local/runs/a"),
            "the task keeps the run that opened it: {task}"
        );
        let again = check::alert(
            &board,
            &[judged("plans.open", 900)],
            &run_path("c"),
            asked(),
            &Lines(None),
        )
        .unwrap();
        assert_eq!(again.tasks, Vec::<String>::new());
    }

    #[test]
    fn a_command_still_over_after_the_grace_files_one_question_and_one_within_the_grace_files_none()
    {
        let scenario = personal();
        let board = open(&scenario);
        let over = [judged("plans.open", 300)];
        check::alert(&board, &over, &run_path("a"), asked(), &Lines(Some(1_000))).unwrap();
        let within =
            check::alert(&board, &over, &run_path("b"), asked(), &Lines(Some(100))).unwrap();
        assert_eq!(within.questions, Vec::<String>::new());
        let past = check::alert(&board, &over, &run_path("c"), asked(), &Lines(Some(101))).unwrap();
        assert_eq!(
            past.questions,
            ["/projects/board/questions/board-speed-plans-open"]
        );
        let second =
            check::alert(&board, &over, &run_path("d"), asked(), &Lines(Some(500))).unwrap();
        assert_eq!(second.questions, Vec::<String>::new());
        let question: Value = serde_json::from_str(
            &text(&board, "/projects/board/questions/board-speed-plans-open").unwrap(),
        )
        .unwrap();
        assert_eq!(question["status"], "open");
    }

    #[test]
    fn two_checks_at_once_open_one_task() {
        let scenario = personal();
        let over = [judged("plans.open", 300)];
        let opened: Vec<_> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2)
                .map(|index| {
                    let root = scenario.workdir().to_path_buf();
                    let over = &over;
                    scope.spawn(move || {
                        let board = open_at(&root);
                        check::alert(
                            &board,
                            over,
                            &run_path(&format!("r{index}")),
                            asked(),
                            &Lines(None),
                        )
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
    fn the_run_record_and_the_suite_pass_their_schemas_and_the_suite_is_a_benchmark() {
        let scenario = personal();
        let board = open(&scenario);
        let suite = check::local_suite();
        assert_eq!(suite["kind"], "benchmark");
        let host = check::Host {
            name: Some("studio".to_owned()),
            os: "macos",
            arch: "aarch64",
            cpus: 14,
        };
        let judged: Vec<_> = (BUDGETS.iter())
            .map(|budget| check::judge(*budget, None))
            .chain([judged("plans.open", 300), judged("get", 5)])
            .collect();
        let run = check::run_record(&judged, "2026-11-23T12:00:00Z", "0.0.0+abc", &host);
        assert_eq!(run["measures"], json!({"over_budget": 1, "killed": 0}));
        assert_eq!(
            run["setup"],
            "board timing check --record on the machine studio"
        );
        for (path, record) in [
            ("/suites/board-speed-local", suite),
            ("/suites/board-speed-local/runs/a", run),
        ] {
            let path = path.parse().unwrap();
            board
                .put(
                    &path,
                    board_ops::Expect::Absent,
                    record.to_string().as_bytes(),
                )
                .unwrap();
        }
    }
}
