//! The command line's own contract: bounded output, help that ends in
//! examples, and errors that say what to try next (STANDARDS.md 23 to 25).

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use super::*;
use crate::run::Cli;

/// Every command that runs, as the words that name it, such as `plugin find`.
fn leaves() -> Vec<(String, clap::Command)> {
    fn walk(prefix: &str, command: &clap::Command, found: &mut Vec<(String, clap::Command)>) {
        for sub in command
            .get_subcommands()
            .filter(|sub| sub.get_name() != "help")
        {
            let name = format!("{prefix}{}", sub.get_name()).trim().to_owned();
            if sub.has_subcommands() {
                walk(&format!("{name} "), sub, found);
            } else {
                found.push((name, sub.clone()));
            }
        }
    }
    let mut found = Vec::new();
    walk("", &Cli::command(), &mut found);
    found
}

/// The arguments of a shell command line: words split on spaces, with
/// single or double quotes holding a word together.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for character in line.chars() {
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
            }
            (Some(open), _) if character == open => quote = None,
            (None, ' ') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(character);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// How clap takes a command line written in help or a hint: `Ok` when it
/// parses or asks for help, and the error kind otherwise.
fn parses(line: &str) -> Result<(), ErrorKind> {
    match Cli::try_parse_from(words(line)) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::DisplayHelp => Ok(()),
        Err(error) => Err(error.kind()),
    }
}

/// Each `board` command an example line shows, without a redirect, a pipe
/// or the description after two spaces.
fn examples(help: &str) -> Vec<String> {
    help.lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|line| line.starts_with("board "))
        .map(|line| line.split("  ").next().unwrap_or(line))
        .flat_map(|line| line.split(" | "))
        .map(|command| command.split(" < ").next().unwrap_or(command).to_owned())
        .collect()
}

#[test]
fn every_command_argument_and_option_has_help() {
    let mut missing = Vec::new();
    let mut commands = vec![("board".to_owned(), Cli::command())];
    while let Some((name, command)) = commands.pop() {
        if command.get_about().is_none() {
            missing.push(name.clone());
        }
        for argument in command.get_arguments() {
            if argument.get_help().is_none() && argument.get_id() != "help" {
                missing.push(format!("{name} {}", argument.get_id()));
            }
        }
        for sub in command
            .get_subcommands()
            .filter(|sub| sub.get_name() != "help")
        {
            commands.push((format!("{name} {}", sub.get_name()), sub.clone()));
        }
    }
    assert!(missing.is_empty(), "no help: {missing:?}");
}

#[test]
fn every_command_that_runs_ends_its_help_in_examples_that_parse() {
    for (name, command) in leaves() {
        let help = command
            .get_after_help()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(help.contains("Examples:\n"), "{name} has no examples");
        let shown = examples(&help);
        assert!(
            shown
                .iter()
                .any(|example| example.starts_with(&format!("board {name}"))),
            "{name}'s examples do not run it: {shown:?}"
        );
        for example in shown {
            assert_eq!(parses(&example), Ok(()), "{example}");
        }
    }
    let root = Cli::command().get_after_help().unwrap().to_string();
    assert!(examples(&root).len() >= 3, "{root}");
    for example in examples(&root) {
        assert_eq!(parses(&example), Ok(()), "{example}");
    }
}

#[test]
fn every_command_group_ends_its_help_in_examples_of_its_commands_that_parse() {
    for group in Cli::command()
        .get_subcommands()
        .filter(|command| command.has_subcommands())
    {
        let name = group.get_name();
        let help = group
            .get_after_help()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(help.contains("Examples:\n"), "{name} has no examples");
        let shown = examples(&help);
        for sub in group
            .get_subcommands()
            .filter(|sub| sub.get_name() != "help")
        {
            let words = format!("board {name} {}", sub.get_name());
            assert!(
                shown.iter().any(|example| example.starts_with(&words)),
                "{name}'s examples do not run {words}: {shown:?}"
            );
        }
        for example in shown {
            assert_eq!(parses(&example), Ok(()), "{example}");
        }
    }
}

#[test]
fn the_start_here_block_says_that_put_writes_and_commits() {
    let root = Cli::command().get_after_help().unwrap().to_string();
    let put = root
        .lines()
        .find(|line| line.trim_start().starts_with("board put "))
        .unwrap_or_else(|| panic!("no put in {root}"));
    assert!(put.contains("commit"), "{put}");
}

#[test]
fn an_example_that_names_a_missing_command_does_not_parse_and_a_real_one_does() {
    assert_eq!(parses("board chek"), Err(ErrorKind::InvalidSubcommand));
    assert_eq!(parses("board check"), Ok(()));
    assert_eq!(
        examples("  board get /a | board put /b < x.json  print"),
        ["board get /a", "board put /b"]
    );
    assert_eq!(
        words(r#"board query "SELECT 'a b'""#),
        ["board", "query", "SELECT 'a b'"]
    );
}

/// Every hint, generic and with every value it can name filled in, names
/// only commands that parse; a generic hint parses once its placeholders
/// are filled, and a filled one parses as it is.
#[test]
fn every_command_a_hint_names_parses() {
    let placeholders = |command: &str| {
        command
            .replace("<plugin>.<capability>", "tasks.due")
            .replace("<command>", "get")
            .replace("<path>", "/a")
            .replace("<name>", "due")
    };
    let known = Fields {
        path: Some("/tasks/a".parse().unwrap()),
        blob: Some(ObjectId::Sha1([1; 20])),
        pointer: Some("/title".to_owned()),
        record_type: Some("lr/task/v1".parse().unwrap()),
        schema: Some(".board/types/lr/task/v1.schema.json".to_owned()),
        plugin: Some("tasks".to_owned()),
        capability: Some("due".to_owned()),
        directory: Some("tasks".to_owned()),
        known: Some(board_model::Known::Columns(
            std::collections::BTreeMap::new(),
        )),
        found: Some(board_model::Found::Blob(ObjectId::Sha1([1; 20]))),
        refusals: Vec::new(),
    };
    let no_blob = Fields {
        blob: None,
        found: Some(board_model::Found::Nothing),
        ..known.clone()
    };
    let tables = Fields {
        known: Some(board_model::Known::Tables(vec!["entries".to_owned()])),
        ..Fields::default()
    };
    let mut filled = 0;
    for fields in [Fields::default(), known, no_blob, tables] {
        for code in Code::ALL {
            let generic = code.hint(&Fields::default());
            let hint = code.hint(&fields);
            assert!(!hint.is_empty() && !hint.ends_with('.'), "{code}: {hint}");
            filled += usize::from(hint != generic);
            for (index, quoted) in hint.split('`').enumerate() {
                if index % 2 == 1 && quoted.starts_with("board ") {
                    let command = if hint == generic {
                        placeholders(quoted)
                    } else {
                        assert!(!quoted.contains('<'), "{code}: {quoted}");
                        quoted.to_owned()
                    };
                    // A hint may name a command without its arguments.
                    let parsed = parses(&command);
                    assert!(
                        matches!(parsed, Ok(()) | Err(ErrorKind::MissingRequiredArgument)),
                        "{code}: {command}: {parsed:?}"
                    );
                }
            }
        }
    }
    assert!(filled >= 20, "only {filled} hints were filled in");
}

/// The commands that list entries take `--limit` and `--all`; every other
/// command is named here as not listing, so a new one has to be placed.
#[test]
fn every_listing_command_takes_limit_and_all() {
    let listing = ["check", "ls", "refs", "plugin list", "plugin find"];
    let other = [
        "init",
        "get",
        "put",
        "mount update",
        "plugin enable",
        "call",
        "query",
    ];
    for (name, command) in leaves() {
        let takes = |flag: &str| {
            command
                .get_arguments()
                .any(|argument| argument.get_long() == Some(flag))
        };
        if listing.contains(&name.as_str()) {
            assert!(takes("limit") && takes("all"), "{name}");
        } else {
            assert!(other.contains(&name.as_str()), "{name} is not placed");
        }
    }
    assert!(leaves().iter().any(|(name, command)| {
        name == "query"
            && command
                .get_arguments()
                .any(|argument| argument.get_long() == Some("limit"))
    }));
}

#[test]
fn a_misspelt_command_is_a_coded_error_with_a_suggestion_and_the_right_one_runs() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["chek"]);
    assert_eq!(refused.code, 2);
    assert!(refused.stdout.is_empty());
    assert_eq!(
        text(&refused.stderr),
        "error: board::usage::invalid: unrecognized subcommand 'chek'\n\
         hint: a similar subcommand exists: 'check'; run `board --help` for the commands, \
         or `board <command> --help` for one command's arguments\n"
    );
    assert_eq!(board_in(scenario.workdir(), &["check"]).code, 0);
}

#[test]
fn a_group_without_its_command_is_a_coded_error_with_its_help_and_with_one_runs() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["plugin"]);
    assert_eq!(refused.code, 2);
    let stderr = text(&refused.stderr);
    assert!(
        stderr.starts_with(
            "error: board::usage::invalid: board plugin needs a command\n\nWork with plugins\n"
        ),
        "{stderr}"
    );
    assert!(stderr.contains("  list  "), "{stderr}");
    assert!(
        stderr.ends_with("\nhint: run `board plugin --help` for its usage and examples\n"),
        "{stderr}"
    );
    assert_eq!(board_in(scenario.workdir(), &["plugin", "list"]).code, 0);
}

#[test]
fn a_refused_command_line_is_an_error_document_when_json_is_asked_for() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["get", "--json"]);
    assert_eq!(refused.code, 2);
    assert!(refused.stderr.is_empty());
    let error = json(&refused);
    assert_eq!(error["$type"], "board/error/v1");
    assert_eq!(error["code"], "board::usage::invalid");
    assert_eq!(
        error["message"],
        "the following required arguments were not provided: <PATH>"
    );
    assert_eq!(
        error["hint"],
        "run `board get --help` for its usage and examples"
    );
    let admitted = board_in(scenario.workdir(), &["get", "/projects/p", "--json"]);
    assert_eq!(admitted.code, 0);
    assert_eq!(json(&admitted)["$type"], "board/get-report/v1");
}

#[test]
fn a_misspelt_command_in_json_suggests_the_right_one_and_the_right_one_runs() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["--json", "chek"]);
    assert_eq!(refused.code, 2);
    let refused = json(&refused);
    assert_eq!(refused["code"], "board::usage::invalid");
    assert_eq!(refused["message"], "unrecognized subcommand 'chek'");
    assert!(
        refused["hint"]
            .as_str()
            .unwrap()
            .starts_with("a similar subcommand exists: 'check'; "),
        "{refused}"
    );
    let admitted = board_in(scenario.workdir(), &["--json", "check"]);
    assert_eq!(admitted.code, 0);
    assert_eq!(json(&admitted)["$type"], "board/check-report/v1");
}

/// Each refused command line names the value at fault, and a corrected
/// twin of it runs.
#[test]
fn a_refused_command_line_names_the_value_at_fault_and_its_corrected_twin_runs() {
    let scenario = board();
    let cases: [(&[&str], &str, &str, &[&str]); 6] = [
        (
            &["ls", "--limt", "5"],
            "unexpected argument '--limt' found",
            "a similar argument exists: '--limit'; run `board ls --help` for its usage and examples",
            &["ls", "--limit", "5"],
        ),
        (
            &["ls", "--limit", "0"],
            "invalid value '0' for '--limit <LIMIT>': must be a whole number from 1 to 10000",
            "run `board ls --help` for its usage and examples",
            &["ls", "--limit", "1"],
        ),
        (
            &["get", "projects/p"],
            "invalid value 'projects/p' for '<PATH>': ",
            "a logical path starts with / and leaves out .json: try `board --json get /projects/p`",
            &["get", "/projects/p"],
        ),
        (
            &["ls", "--limit", "5", "--all"],
            "the argument '--limit <LIMIT>' cannot be used with '--all'",
            "run `board ls --help` for its usage and examples",
            &["ls", "--all"],
        ),
        (
            &["ls", "--limit"],
            "a value is required for '--limit <LIMIT>' but none was supplied",
            "run `board ls --help` for its usage and examples",
            &["ls", "--limit", "1"],
        ),
        (
            &["ls", "-x"],
            "unexpected argument '-x' found",
            "to pass '-x' as a value, use '-- -x'; run `board ls --help` for its usage and examples",
            &["ls"],
        ),
    ];
    for (refused, message, hint, admitted) in cases {
        let output = board_in(scenario.workdir(), &[&["--json"], refused].concat());
        assert_eq!(output.code, 2, "{refused:?}");
        let error = json(&output);
        assert_eq!(error["code"], "board::usage::invalid", "{refused:?}");
        let said = error["message"].as_str().unwrap();
        assert!(said.starts_with(message), "{refused:?}: {said}");
        assert_eq!(error["hint"], hint, "{refused:?}");
        // The corrected command line repeats the arguments, so it drops --json.
        let text_form = text(&board_in(scenario.workdir(), refused).stderr);
        assert_eq!(
            text_form,
            format!(
                "error: board::usage::invalid: {said}\nhint: {}\n",
                hint.replace("board --json ", "board ")
            )
        );
        assert_eq!(
            board_in(scenario.workdir(), admitted).code,
            0,
            "{admitted:?}"
        );
    }
}

/// A board with `count` records under /notes.
fn notes(count: usize) -> Scenario {
    let scenario = board();
    for index in 0..count {
        scenario.write(
            &format!("notes/n{index:03}.json"),
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    }
    scenario
}

#[test]
fn a_listing_past_its_limit_says_what_it_left_out_and_one_within_it_says_nothing() {
    let scenario = notes(3);
    let cache = tempfile::tempdir().unwrap();
    let board_in =
        |directory, arguments: &[&str]| board_cached(directory, Some(cache.path()), arguments);
    let cut = board_in(scenario.workdir(), &["ls", "/notes", "--limit", "2"]);
    assert_eq!(cut.code, 0);
    assert_eq!(text(&cut.stdout).lines().count(), 2);
    assert_eq!(
        text(&cut.stderr),
        "note: showing 2 of 3 entries; --limit <n> (at most 10000) or --all shows more\n"
    );
    let whole = board_in(scenario.workdir(), &["ls", "/notes", "--limit", "3"]);
    assert_eq!(text(&whole.stdout).lines().count(), 3);
    assert_eq!(text(&whole.stderr), "");
    let report = json(&board_in(
        scenario.workdir(),
        &["ls", "/notes", "--limit", "2", "--json"],
    ));
    assert_eq!(report["entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        (report["total"].clone(), report["truncated"].clone()),
        (3.into(), true.into())
    );
}

#[test]
fn a_listing_shows_one_hundred_entries_unless_asked_for_all() {
    let scenario = notes(101);
    let cache = tempfile::tempdir().unwrap();
    let board_in =
        |directory, arguments: &[&str]| board_cached(directory, Some(cache.path()), arguments);
    let limited = board_in(scenario.workdir(), &["ls", "/notes"]);
    assert_eq!(text(&limited.stdout).lines().count(), 100);
    let all = board_in(scenario.workdir(), &["ls", "/notes", "--all"]);
    assert_eq!(text(&all.stdout).lines().count(), 101);
    assert_eq!(text(&all.stderr), "");
}

#[test]
fn limit_and_all_together_are_refused_and_each_alone_is_admitted() {
    let scenario = board();
    let run = |arguments: &[&str]| board_in(scenario.workdir(), arguments).code;
    assert_eq!(run(&["ls", "--limit", "5", "--all"]), 2);
    let refused = json(&board_in(
        scenario.workdir(),
        &["ls", "--limit", "5", "--all", "--json"],
    ));
    assert_eq!(refused["code"], "board::usage::invalid");
    assert_eq!(run(&["ls", "--limit", "5"]), 0);
    assert_eq!(run(&["ls", "--all"]), 0);
}

#[test]
fn check_lists_degraded_entries_first_so_a_limit_never_hides_one() {
    let scenario = notes(3);
    scenario.write("zz/bad.json", Item::File("{"));
    let output = board_in(scenario.workdir(), &["check", "--limit", "1"]);
    assert_eq!(output.code, 1);
    assert_eq!(
        text(&output.stdout),
        "degraded board::json::parse /zz/bad \
         is invalid: json does not parse: EOF while parsing an object at line 1 column 1\n"
    );
    let stderr = text(&output.stderr);
    let note = stderr.lines().last().unwrap();
    assert!(
        note.starts_with("note: showing 1 of 8 entries, 1 of them degraded; "),
        "{stderr}"
    );
    let report = json(&board_in(
        scenario.workdir(),
        &["check", "--limit", "1", "--json"],
    ));
    assert_eq!(report["degraded_total"], 1);
    assert_eq!(report["total"], 8);
    assert_eq!(report["entries"].as_array().unwrap().len(), 1);
    assert_eq!(report["entries"][0]["path"], "zz/bad.json");
    assert_eq!(report["entries"][0]["status"], "degraded");
}

#[test]
fn an_empty_listing_says_so_and_a_full_one_does_not() {
    let scenario = board();
    let cache = tempfile::tempdir().unwrap();
    let refs = || {
        board_cached(
            scenario.workdir(),
            Some(cache.path()),
            &["refs", "/projects/p"],
        )
    };
    let empty = refs();
    assert_eq!(empty.code, 0);
    assert_eq!(
        text(&empty.stderr),
        "note: the record makes no references, and none reach it\n"
    );
    scenario.write(
        "tasks/a.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
    );
    let full = refs();
    assert_eq!(text(&full.stdout), "to /tasks/a /project\n");
    assert_eq!(text(&full.stderr), "");
}

#[test]
fn refs_counts_a_line_per_pointer_so_a_limit_cuts_within_a_record() {
    let scenario = board();
    scenario
        .write(
            ".board/types/lr/pair/v1.schema.json",
            Item::File(
                r#"{"type": "object", "properties": {
                    "a": {"type": "string", "format": "board-ref"},
                    "b": {"type": "string", "format": "board-ref"}}}"#,
            ),
        )
        .write(
            "pairs/x.json",
            Item::File(r#"{"$type": "lr/pair/v1", "a": "/projects/p", "b": "/projects/p"}"#),
        );
    let cache = tempfile::tempdir().unwrap();
    let refs = |arguments: &[&str]| board_cached(scenario.workdir(), Some(cache.path()), arguments);
    let cut = refs(&["refs", "/projects/p", "--limit", "1"]);
    assert_eq!(text(&cut.stdout), "to /pairs/x /a\n");
    assert_eq!(
        text(&cut.stderr),
        "note: showing 1 of 2 references from and to the record; \
         --limit <n> (at most 10000) or --all shows more\n"
    );
    let report = json(&refs(&["refs", "/projects/p", "--limit", "1", "--json"]));
    assert_eq!(report["to"][0]["pointers"], serde_json::json!(["/a"]));
    assert_eq!(
        (report["total"].clone(), report["truncated"].clone()),
        (2.into(), true.into())
    );
    let whole = refs(&["refs", "/projects/p", "--limit", "2"]);
    assert_eq!(text(&whole.stdout), "to /pairs/x /a\nto /pairs/x /b\n");
    assert_eq!(text(&whole.stderr), "");
}

/// A hub that references one record and is reached by two: x with one
/// pointer, y with two. Its listing has four lines in this order.
fn hub() -> Scenario {
    let scenario = board();
    let pair = |path: &str, record: &str| {
        scenario.write(path, Item::File(record));
    };
    pair(
        ".board/types/lr/pair/v1.schema.json",
        r#"{"type": "object", "properties": {
            "a": {"type": "string", "format": "board-ref"},
            "b": {"type": "string", "format": "board-ref"}}}"#,
    );
    pair("hub.json", r#"{"$type": "lr/pair/v1", "a": "/projects/p"}"#);
    pair("x.json", r#"{"$type": "lr/pair/v1", "a": "/hub"}"#);
    pair(
        "y.json",
        r#"{"$type": "lr/pair/v1", "a": "/hub", "b": "/hub"}"#,
    );
    scenario
}

#[test]
fn refs_spends_its_limit_on_references_made_then_on_each_reaching_pointer_in_turn() {
    let scenario = hub();
    let cache = tempfile::tempdir().unwrap();
    let refs = |limit: &str| {
        board_cached(
            scenario.workdir(),
            Some(cache.path()),
            &["refs", "/hub", "--limit", limit],
        )
    };
    let lines = [
        "from /a /projects/p ok\n",
        "to /x /a\n",
        "to /y /a\n",
        "to /y /b\n",
    ];
    for limit in 1..=4 {
        let output = refs(&limit.to_string());
        assert_eq!(text(&output.stdout), lines[..limit].concat(), "{limit}");
        let note = if limit < 4 {
            format!(
                "note: showing {limit} of 4 references from and to the record; \
                 --limit <n> (at most 10000) or --all shows more\n"
            )
        } else {
            String::new()
        };
        assert_eq!(text(&output.stderr), note, "{limit}");
    }
}

#[test]
fn a_refs_limit_that_ends_with_one_record_leaves_the_next_out_of_the_json() {
    let scenario = hub();
    let cache = tempfile::tempdir().unwrap();
    let refs = |limit: &str| {
        json(&board_cached(
            scenario.workdir(),
            Some(cache.path()),
            &["refs", "/hub", "--limit", limit, "--json"],
        ))
    };
    let cut = refs("2");
    assert_eq!(cut["to"].as_array().unwrap().len(), 1);
    assert_eq!(cut["to"][0]["path"], "x.json");
    let whole = refs("4");
    assert_eq!(whole["to"].as_array().unwrap().len(), 2);
    assert_eq!(whole["truncated"], false);
}

/// A query cut short says so in text, where only stderr can carry it, and
/// not in JSON, which carries `truncated`; a whole one says nothing.
#[test]
fn a_query_cut_short_notes_it_on_stderr_only_in_text_and_a_whole_one_does_not() {
    let scenario = notes(3);
    let cache = tempfile::tempdir().unwrap();
    let query = |arguments: &[&str]| {
        let sql = "SELECT path FROM entries WHERE path LIKE 'notes/%'";
        board_cached(
            scenario.workdir(),
            Some(cache.path()),
            &[&["query", sql], arguments].concat(),
        )
    };
    assert_eq!(
        text(&query(&["--limit", "2"]).stderr),
        "note: showing the first 2 rows, and more matched; \
         --limit <n> (at most 10000) shows more\n"
    );
    assert_eq!(text(&query(&["--limit", "2", "--json"]).stderr), "");
    assert_eq!(text(&query(&["--limit", "3"]).stderr), "");
}

#[test]
fn a_json_path_that_fails_for_another_reason_gets_no_hint_and_a_missing_one_does() {
    let scenario = board();
    scenario.write(
        "notes.json/a.json",
        Item::File(r#"{"$type": "lr/project/v1"}"#),
    );
    let directory = board_in(scenario.workdir(), &["get", "/notes.json"]);
    assert_eq!(directory.code, 1);
    assert_eq!(
        text(&directory.stderr).lines().next().unwrap(),
        "error: board::record::not_a_record: /notes.json is not a record"
    );
    assert!(!text(&directory.stderr).contains("leaves out .json"));
    let missing = board_in(scenario.workdir(), &["get", "/other.json"]);
    assert!(text(&missing.stderr).contains("hint: a logical path leaves out .json"));
}

#[test]
fn a_path_given_with_json_is_not_found_with_a_hint_and_without_it_is_found() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["get", "/projects/p.json"]);
    assert_eq!(refused.code, 1);
    assert_eq!(
        text(&refused.stderr),
        "error: board::path::not_found: nothing is at /projects/p.json\n\
         hint: a logical path leaves out .json: try `board get /projects/p`\n"
    );
    assert_eq!(
        board_in(scenario.workdir(), &["get", "/projects/p"]).code,
        0
    );
}

#[test]
fn a_record_whose_logical_path_ends_in_json_round_trips_through_get_and_put() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    scenario.write(
        "tasks/a.json.json",
        Item::File(r#"{"$type": "lr/task/v1", "title": "a"}"#),
    );
    let got = board_in(scenario.workdir(), &["get", "/tasks/a.json"]);
    assert_eq!(got.code, 0, "{}", text(&got.stderr));
    let changed = text(&got.stdout).replace("\"a\"", "\"b\"");
    let put = board_with(
        scenario.workdir(),
        None,
        &["put", "/tasks/a.json"],
        changed.as_bytes(),
    );
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
    assert!(!scenario.workdir().join("tasks/a.json").exists());
    let again = board_in(scenario.workdir(), &["get", "/tasks/a.json", "--json"]);
    assert_eq!(json(&again)["record"]["title"], "b");
}

#[test]
fn a_record_its_schema_refuses_names_the_field_and_why_and_a_valid_one_is_written() {
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let put = |record: &str| {
        board_with(
            scenario.workdir(),
            None,
            &["put", "/tasks/a"],
            record.as_bytes(),
        )
    };
    let refused = put(r#"{"$type": "lr/task/v1", "title": 5}"#);
    assert_eq!(refused.code, 1);
    assert!(
        text(&refused.stderr).starts_with(
            "error: board::record::invalid: the record for /tasks/a \
             fails its schema at \"/title\": 5 is not of type \"string\"\nhint: "
        ),
        "{}",
        text(&refused.stderr)
    );
    assert_eq!(put(r#"{"$type": "lr/task/v1", "title": "a"}"#).code, 0);
}

#[test]
fn check_names_an_entry_by_its_logical_path_and_one_without_or_sharing_it_by_its_file() {
    let scenario = board();
    scenario
        .write("zz/bad.json", Item::File("{"))
        .write(".json", Item::File("{}"))
        .write("x.json", Item::File(r#"{"$type": "lr/project/v1"}"#))
        .write("x", Item::File(""));
    let output = board_in(scenario.workdir(), &["check"]);
    let stdout = text(&output.stdout);
    for line in [
        "degraded board::json::parse /zz/bad \
         is invalid: json does not parse: EOF while parsing an object at line 1 column 1",
        "degraded board::path::malformed .json",
        "degraded board::path::shadowed x",
        "degraded board::path::shadowed x.json",
        "ok /projects/p",
        "ok /.board/board",
    ] {
        assert!(
            stdout.lines().any(|shown| shown == line),
            "{line}: {stdout}"
        );
    }
    let report = json(&board_in(scenario.workdir(), &["check", "--json"]));
    let bad = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["logical"] == "/zz/bad")
        .unwrap();
    assert_eq!(bad["path"], "zz/bad.json");
}

#[test]
fn a_path_refused_by_its_form_hints_the_logical_path_and_that_command_runs() {
    let scenario = board();
    for given in ["projects/p.json", "projects/p"] {
        let refused = board_in(scenario.workdir(), &["get", given]);
        assert_eq!(refused.code, 2);
        let stderr = text(&refused.stderr);
        assert!(
            stderr.starts_with("error: board::usage::invalid: "),
            "{stderr}"
        );
        assert!(
            stderr.ends_with(
                "hint: a logical path starts with / and leaves out .json: \
                 try `board get /projects/p`\n"
            ),
            "{stderr}"
        );
    }
    let refused = json(&board_in(
        scenario.workdir(),
        &["refs", "projects/p.json", "--limit", "5", "--json"],
    ));
    assert_eq!(refused["code"], "board::usage::invalid");
    assert!(
        refused["hint"]
            .as_str()
            .unwrap()
            .ends_with("try `board refs /projects/p --limit 5 --json`"),
        "{refused}"
    );
    assert_eq!(
        board_in(scenario.workdir(), &["get", "/projects/p"]).code,
        0
    );
}

#[test]
fn a_refused_value_that_is_not_a_path_keeps_the_help_hint_and_a_valid_one_runs() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["ls", "--limit", "0"]);
    assert_eq!(refused.code, 2);
    assert!(
        text(&refused.stderr).ends_with("hint: run `board ls --help` for its usage and examples\n"),
        "{}",
        text(&refused.stderr)
    );
    assert_eq!(
        board_in(scenario.workdir(), &["ls", "--limit", "1"]).code,
        0
    );
}

#[test]
fn a_query_naming_a_missing_table_hints_the_tables_there_are_and_a_real_one_runs() {
    let scenario = board();
    let refused = board_in(scenario.workdir(), &["query", "SELECT * FROM entires"]);
    assert_eq!(refused.code, 1);
    let stderr = text(&refused.stderr);
    assert!(
        stderr.starts_with(
            "error: board::query::invalid: the query cannot run: no such table: entires\n\
             hint: the tables and views are "
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(" entries, ") && stderr.contains(" records, "),
        "{stderr}"
    );
    let refused = json(&board_in(
        scenario.workdir(),
        &["query", "SELECT * FROM entires", "--json"],
    ));
    assert_eq!(refused["code"], "board::query::invalid");
    assert!(refused["fields"]["tables"].is_array());
    let admitted = board_in(scenario.workdir(), &["query", "SELECT * FROM entries"]);
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_query_naming_a_missing_column_hints_the_columns_of_its_tables_and_a_real_one_runs() {
    let scenario = board();
    let refused = json(&board_in(
        scenario.workdir(),
        &["query", "SELECT logcal FROM entries", "--json"],
    ));
    assert_eq!(refused["code"], "board::query::invalid");
    assert!(refused["fields"]["columns"]["entries"].is_array());
    let hint = refused["hint"].as_str().unwrap();
    assert!(
        hint.starts_with("the columns are entries: source, path, logical, "),
        "{hint}"
    );
    let unnamed = json(&board_in(
        scenario.workdir(),
        &["query", "SELECT logcal", "--json"],
    ));
    assert_eq!(unnamed["fields"], json!({ "columns": {} }));
    assert_eq!(
        unnamed["hint"],
        "run `board query \"SELECT name, sql FROM sqlite_master\"` for every table and its columns"
    );
    let admitted = board_in(
        scenario.workdir(),
        &["query", "SELECT logical FROM entries"],
    );
    assert_eq!(admitted.code, 0);
}

#[test]
fn a_query_with_a_syntax_error_has_a_hint_that_says_to_fix_it_and_a_valid_one_runs() {
    let scenario = board();
    let refused = json(&board_in(
        scenario.workdir(),
        &["query", "SELEC 1", "--json"],
    ));
    assert_eq!(refused["code"], "board::query::invalid");
    let hint = Code::QueryInvalid.hint(&Fields::default());
    assert_eq!(refused["hint"], hint);
    assert!(hint.starts_with("fix the statement"), "{hint}");
    assert_eq!(board_in(scenario.workdir(), &["query", "SELECT 1"]).code, 0);
}

#[test]
fn version_prints_the_binary_name_and_version() {
    let output = board_in(Path::new("."), &["--version"]);
    assert_eq!(output.code, 0);
    assert_eq!(
        text(&output.stdout),
        format!("{BINARY_NAME} {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_says_what_a_record_is_and_that_put_validates_and_commits() {
    let root = text(&board_in(Path::new("."), &["--help"]).stdout);
    assert!(root.contains("`$type`"), "{root}");
    assert!(root.contains(".board/types/"), "{root}");
    let put = text(&board_in(Path::new("."), &["put", "--help"]).stdout);
    assert!(
        put.contains(".board/types/<namespace>/<kind>/v<n>.schema.json"),
        "{put}"
    );
    assert!(put.contains("commit_on_put"), "{put}");
}

/// Each `board` command line in a code span of the documents that describe
/// what exists. PLAN.md describes commands not yet built, so it is left out.
fn documented() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files: Vec<_> = ["AGENTS.md", "README.md", "DOCS.md"]
        .into_iter()
        .map(|name| root.join(name))
        .collect();
    for entry in std::fs::read_dir(root.join("docs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
    let mut found = Vec::new();
    for file in files {
        let markdown = std::fs::read_to_string(&file).unwrap();
        for (index, span) in markdown.split('`').enumerate() {
            if index % 2 == 1 && span.starts_with("board ") {
                found.push((file.display().to_string(), span.to_owned()));
            }
        }
    }
    found
}

/// A documented command line with each placeholder filled and each
/// optional `[...]` part left out.
fn filled(line: &str) -> String {
    let mut kept = String::new();
    let mut depth = 0;
    for character in line.chars() {
        match character {
            '[' => depth += 1,
            ']' => depth -= 1,
            '…' => {}
            _ if depth == 0 => kept.push(character),
            _ => {}
        }
    }
    kept.replace("<plugin>.<capability>", "tasks.due")
        .replace("<key>=<value>", "k=v")
        .replace("<path>", "/a")
        .replace("<name>", "due")
        .replace("<sql>", "x")
}

#[test]
fn every_command_line_the_docs_show_parses() {
    let shown = documented();
    assert!(shown.len() >= 10, "{shown:?}");
    for (file, line) in shown {
        // A doc may name a command without its arguments.
        let parsed = parses(&filled(&line));
        assert!(
            matches!(parsed, Ok(()) | Err(ErrorKind::MissingRequiredArgument)),
            "{file}: {line}: {parsed:?}"
        );
    }
}

#[test]
fn a_documented_command_that_does_not_exist_does_not_parse_and_one_that_does_parses() {
    assert_eq!(
        parses(&filled("board serve")),
        Err(ErrorKind::InvalidSubcommand)
    );
    assert_eq!(parses(&filled("board check [path]")), Ok(()));
    assert_eq!(
        filled("board plugin find [--capability <name>] [--meta <key>=<value>]…"),
        "board plugin find  "
    );
}
