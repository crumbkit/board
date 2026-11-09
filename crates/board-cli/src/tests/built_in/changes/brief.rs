//! `test_brief.py`: `changes.brief`, which gives an agent what it needs to
//! start from the plan the change names.

use super::*;

const CHANGE: &str = "/changes/t";
const CONTROL: &str = "controls/control/v1";
/// The words a brief may hold.
const BRIEF_WORDS: usize = 600;
/// How many left-out rules a brief names before it only counts them.
const LEFT_OUT_NAMED: usize = 20;

/// The words `brief` holds, counted as the brief counts them.
fn spent(brief: &Value) -> usize {
    let shown = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        other => other.to_string(),
    };
    let mut counted: Vec<String> = [
        "brief_line",
        "path",
        "plan",
        "phase",
        "instruction",
        "controls",
    ]
    .iter()
    .map(|key| shown(&brief[*key]))
    .collect();
    if let Some(lifecycle) = brief.get("lifecycle") {
        counted.push(shown(lifecycle));
    }
    for warning in brief
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        counted.push(format!(
            "{} {}",
            shown(&warning["code"]),
            shown(&warning["detail"])
        ));
    }
    let command = &brief["rules_command"];
    if !command.is_null() {
        counted.extend(command["argv"].as_array().unwrap().iter().map(shown));
        counted.push(shown(&command["stdin"]));
    }
    for rule in brief["rules"].as_array().unwrap() {
        let matched: Vec<String> = (rule["matched"].as_array().into_iter().flatten())
            .map(shown)
            .collect();
        counted.push(format!(
            "{} {} {}",
            shown(&rule["control"]),
            shown(&rule["summary"]),
            matched.join(" ")
        ));
    }
    for left in brief["rules_left_out"].as_array().unwrap() {
        counted.push(format!(
            "{} {}",
            shown(&left["control"]),
            shown(&left["reason"])
        ));
    }
    for used in brief["uses"].as_array().unwrap() {
        counted.push(format!("{} {}", shown(&used["ref"]), shown(&used["line"])));
    }
    counted.join(" ").split_whitespace().count()
}

/// The input of `changes.plan` for [`CHANGE`] with `uses` and `paths`.
fn planned(uses: &[&str], paths: &[&str]) -> Value {
    json!({"path": CHANGE, "text": PLAN, "uses": uses, "paths": paths})
}

/// The fixture plan with a Lifecycle section that names a record.
fn lifecycle_named() -> String {
    let (before, _) = PLAN.rsplit_once("## Lifecycle").unwrap();
    format!("{before}## Lifecycle\n\nSee the change's lifecycle record: open, closed.\n")
}

/// The input of `plan` for `text`.
fn naming(text: &str) -> Value {
    json!({"path": CHANGE, "text": text, "uses": [], "paths": PATHS})
}

fn brief(board: &Changes) -> Value {
    board.call("brief", &json!({"path": CHANGE}))
}

/// The controls named in `rules`.
fn named(rules: &Value) -> Vec<String> {
    (rules.as_array().unwrap().iter())
        .map(|rule| rule["control"].as_str().unwrap().to_owned())
        .collect()
}

/// A control on `paths` with `requirement` and `fields`.
fn control(requirement: &str, paths: &[&str], fields: &Value) -> String {
    let body = json!({
        "$type": CONTROL,
        "requirement": requirement,
        "purpose": "p",
        "paths": paths,
    });
    record(body, fields)
}

/// The board with a change and a repository mounted at /repos/x holding
/// `src/a.rs`.
fn with_source() -> Changes {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[("src/a.rs", "a\n")]);
    board
}

/// The controls `board controls for-paths` lists when run as the
/// brief's `rules_command` says.
fn listed(board: &Changes, command: &Value) -> Vec<String> {
    let argv: Vec<&str> = (command["argv"].as_array().unwrap().iter())
        .map(|part| part.as_str().unwrap())
        .collect();
    assert_eq!(argv[0], "board");
    let report = board.json_with(&argv[1..], command["stdin"].as_str().unwrap());
    named(&report["data"]["output"]["controls"])
}

#[test]
fn brief_gives_one_line_for_each_record_the_newest_plan_uses() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write(
        ".board/types/evals/eval/v1.schema.json",
        r#"{"type": "object"}"#,
    );
    board.write(
        "controls/a.json",
        &typed(
            CONTROL,
            &json!({"requirement": "Refusals carry a code.", "purpose": "p"}),
        ),
    );
    board.write(
        "controls/b.json",
        &typed(
            CONTROL,
            &json!({"summary": "Stay small.", "requirement": "Long text.", "purpose": "p"}),
        ),
    );
    board.write(
        "decisions/d.json",
        &typed(
            "plans/decision/v1",
            &json!({"decision": "Plans are records."}),
        ),
    );
    board.write(
        "evals/e.json",
        &typed(
            "evals/eval/v1",
            &json!({"question": "Do trees help?", "verdict": "supported"}),
        ),
    );
    board.write(
        "standards/errors.md",
        "Intro.\n\n## Refusal codes\n\nText.\n",
    );
    board.call_stdin("plan", &planned(&[], &PATHS));
    let uses = [
        "/controls/a",
        "/controls/b",
        "/decisions/d",
        "/evals/e",
        "/standards/errors.md",
    ];
    let newest = board.call_stdin("plan", &planned(&uses, &PATHS));
    let brief = brief(&board);
    assert_eq!(brief["plan"], newest["text"]);
    assert_eq!(
        brief["uses"],
        json!([
            {"ref": "/controls/a", "line": "Refusals carry a code."},
            {"ref": "/controls/b", "line": "Stay small."},
            {"ref": "/decisions/d", "line": "Plans are records."},
            {"ref": "/evals/e", "line": "Do trees help? Verdict: supported."},
            {"ref": "/standards/errors.md", "line": "Refusal codes"},
        ])
    );
    assert_eq!(
        (&brief["left_out"], &brief["phase"]),
        (&json!(0), &json!("plan"))
    );
    assert_eq!(brief["controls"], "board controls check --path /repos/x");
}

#[test]
fn brief_stays_within_600_words_with_100_uses_and_counts_what_it_left_out() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let count = 100;
    let uses: Vec<String> = (0..count)
        .map(|number| format!("/standards/s{number:03}.md"))
        .collect();
    for used in &uses {
        board.write(
            used.trim_start_matches('/'),
            &format!("# {}\n", "word ".repeat(20)),
        );
    }
    let uses: Vec<&str> = uses.iter().map(String::as_str).collect();
    board.call_stdin("plan", &planned(&uses, &PATHS));
    let brief = brief(&board);
    let shown = brief["uses"].as_array().unwrap().len();
    assert_eq!(brief["left_out"], count - shown);
    assert!(0 < shown && shown < count, "{shown}");
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn brief_without_a_plan_record_names_no_plan_and_no_uses() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let brief = brief(&board);
    assert_eq!(
        (&brief["plan"], &brief["uses"], &brief["left_out"]),
        (&Value::Null, &json!([]), &json!(0))
    );
}

#[test]
fn brief_keeps_the_newest_plan_when_a_document_it_uses_is_deleted() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.write("standards/a.md", "# A\n");
    board.write("standards/b.md", "# B\n");
    board.call_stdin("plan", &planned(&["/standards/a.md"], &PATHS));
    let newest = board.call_stdin("plan", &planned(&["/standards/b.md"], &PATHS));
    board.delete("standards/b.md");
    let brief = brief(&board);
    assert_eq!(brief["plan"], newest["text"]);
    assert_eq!(
        brief["uses"],
        json!([{"ref": "/standards/b.md", "line": "unreadable: reaches nothing or is degraded"}])
    );
}

#[test]
fn brief_starts_with_the_line_the_write_guard_reads() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let ran = board.run(&["--json", "call", "changes.brief", "--path", CHANGE], "");
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    // The guard reads the output's first key, which a parsed document's
    // sorted keys would hide, so this reads the printed text.
    let printed = text(&ran.stdout);
    let output = &printed[printed.find(r#""output""#).unwrap()..];
    let opened = &output[output.find('{').unwrap() + 1..];
    let first = opened.trim_start();
    assert!(first.starts_with(r#""brief_line""#), "{printed}");
    assert_eq!(
        json(&ran)["data"]["output"]["brief_line"],
        format!("board-brief: {CHANGE}")
    );
}

/// The golden of what `board changes brief` prints, which the write
/// guard's tests read. A mod's tests import only TypeScript, so the golden
/// is a module whose default export is the printed text as a JSON string.
const BRIEF_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../plugins/changes/claude-code/tests/brief.golden.ts"
);

/// The committed golden holds what an agent sees when it runs `board call
/// changes.brief` without `--json`, so the guard's tests read Board's real
/// output. `SNAPSHOTS=overwrite` rewrites the file.
#[test]
fn the_brief_names_the_lifecycle_text_and_a_1500_word_text_stays_outside_its_600_words() {
    let board = with_source();
    board.call_stdin("plan", &naming(&lifecycle_named()));
    assert_eq!(brief(&board).get("lifecycle"), None);
    let text = "w ".repeat(1_500);
    board.call_stdin(
        "lifecycle",
        &json!({"path": CHANGE, "text": text, "doc": "/repos/x/docs/states.md", "external": []}),
    );
    let found = brief(&board);
    assert_eq!(found["lifecycle"], "/changes/t/lifecycle.md");
    assert!(spent(&found) <= BRIEF_WORDS, "{}: {found}", spent(&found));
}

#[test]
fn the_brief_leaves_out_the_lifecycle_text_of_a_plan_revised_to_none_and_names_it_once_revised_back()
 {
    let board = with_source();
    board.call_stdin("plan", &naming(&lifecycle_named()));
    board.call_stdin(
        "lifecycle",
        &json!({"path": CHANGE, "text": "A table.", "doc": "/repos/x/docs/states.md", "external": []}),
    );
    assert_eq!(brief(&board)["lifecycle"], "/changes/t/lifecycle.md");
    board.call_stdin("plan", &planned(&[], &PATHS));
    assert_eq!(brief(&board).get("lifecycle"), None);
    board.call_stdin("plan", &naming(&lifecycle_named()));
    assert_eq!(brief(&board)["lifecycle"], "/changes/t/lifecycle.md");
}

#[test]
fn the_brief_warns_when_the_board_holds_no_lifecycle_paths_control_and_is_silent_when_it_does() {
    let board = with_source();
    board.call_stdin("plan", &planned(&[], &PATHS));
    let found = brief(&board);
    assert_eq!(found["warnings"][0]["code"], "changes::brief::no_control");
    assert_eq!(
        found["warnings"][0]["detail"],
        "no control /controls/x/lifecycle-paths exists, so only the plan's Lifecycle heading \
         guards what it would list; write the control to name those paths"
    );
    assert!(spent(&found) <= BRIEF_WORDS, "{}: {found}", spent(&found));
    board.write(
        "controls/x/lifecycle-paths.json",
        &control(
            "This code changes state.",
            &["repos/x/src/sync.rs"],
            &json!({}),
        ),
    );
    assert_eq!(brief(&board).get("warnings"), None);
}

#[test]
fn the_brief_of_a_change_on_a_flow_with_no_lifecycle_check_gives_no_warning() {
    let board = Changes::new();
    board.mount_x(&[("src/a.rs", "a\n")]);
    let start = |name: &str, flow: &str| {
        board.call(
            "start",
            &json!({"mount": "/repos/x", "title": name, "name": name, "task": TASK, "flow": flow}),
        );
        board.call("brief", &json!({"path": format!("/changes/{name}")}))
    };
    assert_eq!(start("small-one", "small").get("warnings"), None);
    assert_eq!(
        start("development-one", "development")["warnings"][0]["code"],
        "changes::brief::no_control"
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "brief.golden.ts is a golden, written as the other goldens are"
)]
#[test]
fn the_brief_an_agent_runs_is_the_golden_the_write_guard_tests_read() {
    // Every change `start` writes has a plan, so the fixture has one too.
    // Its record is at plans/1, so the brief names no stamped plan file and
    // nothing in it depends on the machine or the date.
    let board = with_source();
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md"}),
    );
    board.write("changes/t/plan.md", "# T\n");
    let ran = board.run(&["call", "changes.brief", "--path", CHANGE], "");
    assert_eq!(ran.code, 0, "{}", text(&ran.stderr));
    let printed = text(&ran.stdout);
    let instruction = serde_json::from_str::<Value>(&printed).unwrap()["instruction"].clone();
    assert!(
        (instruction.as_str().unwrap()).ends_with(&revised_plan_usage(CHANGE, "/repos/x")),
        "{printed}"
    );
    assert!(
        !printed.contains(&board.root().display().to_string()),
        "{printed}"
    );
    let golden = format!(
        "// What `board changes brief` prints. Written by\n\
         // the_brief_an_agent_runs_is_the_golden_the_write_guard_tests_read in\n\
         // crates/board-cli/src/tests/built_in/changes/brief.rs; run it with\n\
         // SNAPSHOTS=overwrite to rewrite this file.\n\
         export default {}\n",
        serde_json::to_string(&printed).unwrap()
    );
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::write(BRIEF_GOLDEN, &golden).unwrap();
    }
    let committed = std::fs::read_to_string(BRIEF_GOLDEN).unwrap_or_default();
    assert!(
        committed == golden,
        "{BRIEF_GOLDEN} differs from what changes.brief prints; run this test with SNAPSHOTS=overwrite:\n{printed}"
    );
}

#[test]
fn brief_lists_each_control_on_the_planned_paths_once_matched_ones_first_and_what_they_matched() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[("src/a.rs", "a\n"), ("docs/a.md", "a\n")]);
    board.write(
        "controls/rust.json",
        &control("Errors are typed. Always.", &["repos/x/*.rs"], &json!({})),
    );
    board.write(
        "controls/all.json",
        &control(
            "Test first.",
            &["repos/x/*"],
            &json!({"summary": "Write the test first."}),
        ),
    );
    board.write(
        "controls/docs.json",
        &control("Say what exists.", &["repos/x/docs/*"], &json!({})),
    );
    board.write(
        "controls/other.json",
        &control("Elsewhere.", &["repos/y/*"], &json!({})),
    );
    board.call_stdin(
        "plan",
        &planned(&[], &["/repos/x/src/**", "/repos/x/docs/new.md"]),
    );
    let brief = brief(&board);
    assert_eq!(
        brief["rules"],
        json!([
            {"control": "/controls/docs", "summary": "Say what exists.", "matched": ["/repos/x/docs/new.md"]},
            {"control": "/controls/rust", "summary": "Errors are typed.", "matched": ["/repos/x/src/**"]},
            {"control": "/controls/all", "summary": "Write the test first."},
        ])
    );
    assert_eq!(
        (&brief["rules_left_out"], &brief["rules_unnamed"]),
        (&json!([]), &json!(0))
    );
}

#[test]
fn a_version_control_whose_matched_files_pin_no_version_is_left_out_and_says_why() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[
        ("src/a.rs", "a\n"),
        ("mise.toml", "node = \"26\"\n"),
        ("pinned/mise.toml", "rust = \"1.1\"\n"),
    ]);
    let rule =
        json!({"rule": {"check": "version", "at_least": "1.2", "capture": "rust = \"([0-9.]+)\""}});
    board.write(
        "controls/rust.json",
        &control("Pin Rust.", &["repos/x/*mise.toml"], &rule),
    );
    board.write(
        "controls/typed.json",
        &control("Errors are typed.", &["repos/x/*"], &json!({})),
    );
    board.call_stdin(
        "plan",
        &planned(&[], &["/repos/x/src/**", "/repos/x/mise.toml"]),
    );
    let left = brief(&board);
    assert_eq!(named(&left["rules"]), ["/controls/typed"]);
    assert_eq!(
        (&left["rules_left_out"], &left["rules_unnamed"]),
        (
            &json!([{"control": "/controls/rust", "reason": NO_VERSION}]),
            &json!(0)
        )
    );
    assert!(spent(&left) <= BRIEF_WORDS, "{}", spent(&left));
    board.call_stdin(
        "plan",
        &planned(&[], &["/repos/x/src/**", "/repos/x/pinned/**"]),
    );
    let kept = brief(&board);
    assert_eq!(named(&kept["rules"]), ["/controls/rust", "/controls/typed"]);
    assert_eq!(kept["rules_left_out"], json!([]));
}

#[test]
fn a_version_control_is_kept_when_a_planned_file_is_new_or_unreadable_and_dropped_when_it_holds_none()
 {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    // dir/mise.toml is a directory, which Board cannot read as a file.
    board.mount_x(&[("mise.toml", "node = \"26\"\n"), ("dir/mise.toml/a", "a\n")]);
    let rule =
        json!({"rule": {"check": "version", "at_least": "1.2", "capture": "rust = \"([0-9.]+)\""}});
    board.write(
        "controls/rust.json",
        &control("Pin Rust.", &["repos/x/*mise.toml"], &rule),
    );
    let rules_for = |paths: &[&str]| {
        board.call_stdin("plan", &planned(&[], paths));
        let brief = brief(&board);
        (named(&brief["rules"]), brief["rules_left_out"].clone())
    };
    let kept = (vec!["/controls/rust".to_owned()], json!([]));
    assert_eq!(rules_for(&["/repos/x/new/mise.toml"]), kept);
    assert_eq!(rules_for(&["/repos/x/new/**"]), kept);
    assert_eq!(rules_for(&["/repos/x/dir/mise.toml"]), kept);
    assert_eq!(
        rules_for(&["/repos/x/mise.toml"]),
        (
            Vec::<String>::new(),
            json!([{"control": "/controls/rust", "reason": NO_VERSION}])
        )
    );
}

#[test]
fn a_rule_lists_at_most_three_matched_paths_and_counts_the_rest() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.mount_x(&[("a.rs", "a\n")]);
    board.write(
        "controls/rust.json",
        &control("Errors are typed.", &["repos/x/*.rs"], &json!({})),
    );
    let paths = [
        "/repos/x/a.rs",
        "/repos/x/b.rs",
        "/repos/x/c.rs",
        "/repos/x/d.rs",
        "/repos/x/e.md",
    ];
    board.call_stdin("plan", &planned(&[], &paths));
    let rules = brief(&board)["rules"].clone();
    let [rule] = rules.as_array().unwrap().as_slice() else {
        panic!("{rules}");
    };
    assert_eq!(
        (&rule["matched"], &rule["matched_left_out"]),
        (&json!(paths[..3]), &json!(1))
    );
    board.call_stdin("plan", &planned(&[], &paths[2..]));
    let rules = brief(&board)["rules"].clone();
    let [rule] = rules.as_array().unwrap().as_slice() else {
        panic!("{rules}");
    };
    assert_eq!(rule["matched"], json!(paths[2..4]));
    assert!(rule.get("matched_left_out").is_none(), "{rule}");
}

#[test]
fn brief_stays_within_600_words_with_100_rules_names_20_it_left_out_and_counts_the_rest() {
    let board = with_source();
    let count = 100;
    let requirement = format!("{}end.", "word ".repeat(20));
    for number in 0..count {
        board.write(
            &format!("controls/c{number:03}.json"),
            &control(&requirement, &["repos/x/*"], &json!({})),
        );
    }
    board.call_stdin("plan", &planned(&[], &PATHS));
    let brief = brief(&board);
    let shown = named(&brief["rules"]);
    assert!(!shown.is_empty() && shown.len() < count, "{}", shown.len());
    let every: Vec<String> = (0..count)
        .map(|number| format!("/controls/c{number:03}"))
        .collect();
    let mut both = shown.clone();
    both.extend(named_paths(&brief["rules_left_out"]));
    assert_eq!(both, every[..shown.len() + LEFT_OUT_NAMED]);
    assert_eq!(brief["rules_unnamed"], count - shown.len() - LEFT_OUT_NAMED);
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
    let command = &brief["rules_command"];
    assert_eq!(
        *command,
        json!({
            "argv": ["board", "call", "controls.for-paths"],
            "stdin": r#"{"paths": ["/repos/x/**"]}"#,
        })
    );
    assert_eq!(listed(&board, command), every);
}

/// The reason a rule left out for want of room carries.
const NO_ROOM: &str = "no room in the brief's 600 words";
/// The reason a version control whose matched files pin no version carries.
const NO_VERSION: &str = "no version found in the matched files";

/// The controls of the left-out rules `left_out`, each of which was left
/// out for want of room.
fn named_paths(left_out: &Value) -> Vec<String> {
    (left_out.as_array().unwrap().iter())
        .map(|left| {
            assert_eq!(left["reason"], NO_ROOM, "{left}");
            left["control"].as_str().unwrap().to_owned()
        })
        .collect()
}

#[test]
fn the_rules_command_needs_no_shell_quoting_for_a_path_holding_a_quote() {
    let board = with_source();
    board.write(
        "controls/quote.json",
        &control("Quote it.", &["repos/x/src/*"], &json!({})),
    );
    let paths = ["/repos/x/src/it's.rs"];
    board.call_stdin("plan", &planned(&[], &paths));
    let command = brief(&board)["rules_command"].clone();
    let stdin: Value = serde_json::from_str(command["stdin"].as_str().unwrap()).unwrap();
    assert_eq!(stdin, json!({"paths": paths}));
    assert_eq!(listed(&board, &command), ["/controls/quote"]);
}

#[test]
fn a_rule_too_long_to_fit_leaves_room_for_the_shorter_rules_after_it() {
    let board = with_source();
    let count = 120;
    let long = format!("{}end.", "word ".repeat(22));
    for number in 0..count {
        let requirement = if number % 6 == 0 {
            long.as_str()
        } else {
            "Do."
        };
        board.write(
            &format!("controls/c{number:03}.json"),
            &control(requirement, &["repos/x/*"], &json!({})),
        );
    }
    board.call_stdin("plan", &planned(&[], &PATHS));
    let brief = brief(&board);
    let shown = named(&brief["rules"]);
    let every: Vec<String> = (0..count)
        .map(|number| format!("/controls/c{number:03}"))
        .collect();
    let left_out = named_paths(&brief["rules_left_out"]);
    assert!(!left_out.is_empty(), "{brief}");
    let first: usize = left_out[0]
        .strip_prefix("/controls/c")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(first % 6, 0);
    let position = |name: &String| every.iter().position(|each| each == name).unwrap();
    assert!(position(shown.last().unwrap()) > position(&left_out[0]));
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn every_rule_gets_its_summary_before_any_rule_gets_its_matched_paths() {
    let board = with_source();
    let count = 100;
    for number in 0..count {
        board.write(
            &format!("controls/c{number:03}.json"),
            &control("Do it.", &["repos/x/src/*"], &json!({})),
        );
    }
    let paths = [
        "/repos/x/src/a.rs",
        "/repos/x/src/b.rs",
        "/repos/x/src/c.rs",
        "/repos/x/docs/d.md",
    ];
    board.call_stdin("plan", &planned(&[], &paths));
    let brief = brief(&board);
    let every: Vec<String> = (0..count)
        .map(|number| format!("/controls/c{number:03}"))
        .collect();
    assert_eq!(named(&brief["rules"]), every);
    assert_eq!(brief["rules_left_out"], json!([]));
    let with_matched: Vec<bool> = (brief["rules"].as_array().unwrap().iter())
        .map(|rule| rule.get("matched").is_some())
        .collect();
    assert!(with_matched[0]);
    assert!(!with_matched[with_matched.len() - 1]);
    let mut sorted = with_matched.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(with_matched, sorted);
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn many_rules_leave_room_for_the_lines_of_the_records_the_plan_uses() {
    let board = with_source();
    let requirement = format!("{}end.", "word ".repeat(20));
    for number in 0..100 {
        board.write(
            &format!("controls/c{number:03}.json"),
            &control(&requirement, &["repos/x/*"], &json!({})),
        );
    }
    let uses: Vec<String> = (0..5)
        .map(|number| format!("/standards/s{number}.md"))
        .collect();
    for used in &uses {
        board.write(
            used.trim_start_matches('/'),
            &format!("# {}\n", "word ".repeat(20)),
        );
    }
    let uses: Vec<&str> = uses.iter().map(String::as_str).collect();
    board.call_stdin("plan", &planned(&uses, &PATHS));
    let brief = brief(&board);
    let refs: Vec<&str> = (brief["uses"].as_array().unwrap().iter())
        .map(|used| used["ref"].as_str().unwrap())
        .collect();
    assert_eq!(refs, uses);
    assert_ne!(
        brief["rules"].as_array().unwrap().as_slice(),
        &[] as &[Value]
    );
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn brief_without_the_controls_plugin_is_refused_and_with_it_is_admitted() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    board.call_stdin("plan", &planned(&[], &PATHS));
    let config = board.read(".board/board.json");
    board.write(
        ".board/board.json",
        &record(config.clone(), &json!({"plugins": ["changes"]})),
    );
    let refusal = board.refusal("brief", &json!({"path": CHANGE}));
    assert_eq!(
        refusal["code"], "changes::brief::controls_missing",
        "{refusal}"
    );
    board.write(".board/board.json", &config.to_string());
    assert_eq!(brief(&board)["rules"], json!([]));
}

#[test]
fn brief_of_a_plan_without_paths_lists_no_rules() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let brief = brief(&board);
    assert_eq!(
        (
            &brief["rules"],
            &brief["rules_left_out"],
            &brief["rules_command"]
        ),
        (&json!([]), &json!([]), &Value::Null)
    );
}

#[test]
fn rules_that_matched_some_planned_paths_come_before_rules_that_matched_them_all() {
    let board = with_source();
    let requirement = format!("{}end.", "word ".repeat(10));
    for number in 0..300 {
        board.write(
            &format!("controls/a-extra/e{number:03}.json"),
            &control(&requirement, &["repos/x/*"], &json!({})),
        );
    }
    let real: Vec<String> = (0..5)
        .map(|number| format!("/controls/z-real/r{number}"))
        .collect();
    for path in &real {
        board.write(
            &format!("{}.json", path.trim_start_matches('/')),
            &control("Errors are typed.", &["repos/x/src/*"], &json!({})),
        );
    }
    board.call_stdin(
        "plan",
        &planned(&[], &["/repos/x/src/a.rs", "/repos/x/docs/b.md"]),
    );
    let brief = brief(&board);
    let shown = named(&brief["rules"]);
    assert_eq!(shown[..real.len()], real);
    let rest = &shown[real.len()..];
    let mut sorted = rest.to_vec();
    sorted.sort();
    assert_eq!(rest, sorted);
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn the_words_kept_for_left_out_names_go_to_rules_when_every_rule_fits() {
    let board = with_source();
    board.call_stdin("plan", &planned(&[], &PATHS));
    let room = BRIEF_WORDS - spent(&brief(&board));
    // Rules of two words each that fill the room, with no word kept for a
    // name.
    let count = room / 2;
    for number in 0..count {
        board.write(
            &format!("controls/c{number:03}.json"),
            &control("Do.", &["repos/x/*"], &json!({})),
        );
    }
    let brief = brief(&board);
    assert_eq!(brief["rules"].as_array().unwrap().len(), count);
    assert_eq!(
        (&brief["rules_left_out"], &brief["rules_unnamed"]),
        (&json!([]), &json!(0))
    );
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn a_plan_that_leaves_five_words_stays_within_the_limit_and_counts_the_rules_it_cannot_name() {
    let board = with_source();
    let count = 30;
    for number in 0..count {
        board.write(
            &format!("controls/c{number:03}.json"),
            &control("Do.", &["repos/x/*"], &json!({})),
        );
    }
    board.call_stdin("plan", &planned(&[], &PATHS));
    // More planned paths lengthen `rules_command`'s stdin until the fixed
    // parts leave five words.
    let room = BRIEF_WORDS - spent(&brief(&board)) + 2 * count;
    let mut paths: Vec<String> = PATHS.iter().map(|path| (*path).to_owned()).collect();
    paths.extend((0..room - 5).map(|number| format!("/repos/x/f{number}.rs")));
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    board.call_stdin("plan", &planned(&[], &paths));
    let brief = brief(&board);
    let rules = brief["rules"].as_array().unwrap().len();
    let left_out = brief["rules_left_out"].as_array().unwrap().len();
    let unnamed = brief["rules_unnamed"].as_u64().unwrap();
    assert_eq!(rules + left_out + usize::try_from(unnamed).unwrap(), count);
    assert!(unnamed > 0, "{brief}");
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}

#[test]
fn a_use_too_long_to_fit_leaves_room_for_the_shorter_uses_after_it() {
    let board = Changes::new();
    board.change_at("changes/t.json", "plan", &json!({}));
    let uses = ["/standards/a.md", "/standards/long.md", "/standards/b.md"];
    board.write("standards/a.md", "# Short.\n");
    board.write(
        "standards/long.md",
        &format!("# {}\n", "word ".repeat(BRIEF_WORDS)),
    );
    board.write("standards/b.md", "# Short.\n");
    board.call_stdin("plan", &planned(&uses, &PATHS));
    let brief = brief(&board);
    let refs: Vec<&str> = (brief["uses"].as_array().unwrap().iter())
        .map(|used| used["ref"].as_str().unwrap())
        .collect();
    assert_eq!(refs, ["/standards/a.md", "/standards/b.md"]);
    assert_eq!(brief["left_out"], 1);
    assert!(spent(&brief) <= BRIEF_WORDS, "{}", spent(&brief));
}
