//! No hint is a dead end (STANDARDS.md 25): each `board` command a hint
//! names, run where the error happened, succeeds or leads within three hops
//! to a hint that names no command.

use std::path::PathBuf;

use super::cli::words;
use super::plugins::{
    RESULT, TASKS, answering, install, installed, lacking_a_schema, shipping, unwritable_databases,
};
use super::*;

/// The most hints followed from one error.
const HOPS: usize = 3;

/// A command line the property starts from, and the code it fails with.
type Start<'a> = (Code, &'a [&'a str]);

/// Where commands run: a board, its index cache, the plugins installed for
/// it, the depth of plugin calls it is nested in, and stdin. A board
/// `shared` by several starting commands must not be changed by any.
#[derive(Clone)]
struct Place<'a> {
    directory: &'a Path,
    cache: Option<&'a Path>,
    plugins: Option<PathBuf>,
    depth: Option<&'a str>,
    stdin: &'a [u8],
    shared: bool,
}

impl<'a> Place<'a> {
    fn new(directory: &'a Path) -> Self {
        Self {
            directory,
            cache: None,
            plugins: None,
            depth: None,
            stdin: b"",
            shared: false,
        }
    }

    fn shared(&self) -> Self {
        Self {
            shared: true,
            ..self.clone()
        }
    }

    fn with(&self, stdin: &'a [u8]) -> Self {
        Self {
            stdin,
            ..self.clone()
        }
    }

    fn run(&self, arguments: &[String]) -> Output {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let environment = board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            depth: self.depth.map(Into::into),
            ..board_plugin::Environment::default()
        };
        board_environment(
            self.directory,
            self.cache,
            self.plugins.as_deref(),
            environment,
            &arguments,
            self.stdin,
        )
    }

    /// The first chain of hints from `arguments` that is a dead end, adding
    /// each code on the way to `codes`. A starting command must fail with
    /// the code it is `intended` to give. `hinted` is the code of the error
    /// whose hint named `arguments`; failing with it again is a dead end,
    /// unless `arguments` ran `check`. A `check` entry whose hint says to fix
    /// something, "then run" that same check, ends its chain: Board cannot
    /// take the fix for the reader. A degraded entry with no hint is a dead
    /// end.
    fn follow(
        &self,
        arguments: &[String],
        intended: Option<&str>,
        hinted: Option<&str>,
        hops: usize,
        codes: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        // `check` reads as JSON, which pairs each entry with its hint.
        let mut run = arguments.to_vec();
        if run.first().is_some_and(|command| command == "check") && !run.contains(&"--json".into())
        {
            run.push("--json".into());
        }
        let output = self.run(&run);
        let line = format!("board {}", arguments.join(" "));
        if output.code == 0 {
            assert!(intended.is_none(), "{line} succeeded, not {intended:?}");
            assert!(
                !(self.shared && writes(arguments)),
                "{line} changed a board that other starting commands share"
            );
            return None;
        }
        let (failures, report) = failures(&output);
        if let Some(intended) = intended {
            let found = failures.iter().any(|(code, _)| code == intended);
            assert!(found, "{line} gave {failures:?}, not {intended}");
        }
        if failures.is_empty() {
            let code = output.code;
            return Some(vec![format!("{line} -> exit {code}, with no error")]);
        }
        for (code, hint) in failures {
            codes.insert(code.clone());
            let Some(hint) = hint else {
                return Some(vec![format!("{line} -> {code}, with no hint")]);
            };
            let step = format!("{line} -> {code}: {hint}");
            if hinted == Some(code.as_str()) && !report {
                return Some(vec![step]);
            }
            for (command, manual) in commands(&hint) {
                let dead = if unfilled(&command) {
                    Some(Vec::new())
                } else if command.iter().any(|word| word.contains('<'))
                    || (report && manual && command == arguments)
                {
                    None
                } else if hops == HOPS {
                    Some(Vec::new())
                } else {
                    self.follow(&command, None, Some(&code), hops + 1, codes)
                };
                if let Some(mut chain) = dead {
                    chain.insert(0, step);
                    return Some(chain);
                }
            }
        }
        None
    }
}

/// Whether `arguments` changes the board when it succeeds.
fn writes(arguments: &[String]) -> bool {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    matches!(
        words.as_slice(),
        ["put" | "init", ..] | ["mount", "update", ..] | ["plugin", "enable", ..]
    )
}

/// Whether `command` holds a placeholder Board could have filled in. Board
/// cannot know another board's directory or the command a reader meant, so
/// a hint naming only those names no command to run.
fn unfilled(command: &[String]) -> bool {
    command
        .iter()
        .any(|word| word.contains('<') && word != "<dir>" && word != "<command>")
}

/// An error's `hint`, which a plugin's own `code` may lack, since Board
/// gives it none; a Board code without one has no hint.
fn unless_board(code: &str, hint: Option<String>) -> Option<String> {
    hint.or_else(|| (!code.starts_with("board::")).then(String::new))
}

/// The code and hint of each failure `output` reports, and whether it is a
/// `check` report: from an error document or error line, or else from each
/// degraded entry of a report. A `check` entry without a hint has none;
/// another command's entries, such as `plugin list`'s, carry no hint, so
/// each names no command.
fn failures(output: &Output) -> (Vec<(String, Option<String>)>, bool) {
    if let Ok(document) = serde_json::from_slice::<Value>(&output.stdout) {
        if let Some(code) = document["code"].as_str() {
            let hint = document["hint"].as_str().map(str::to_owned);
            return (vec![(code.to_owned(), unless_board(code, hint))], false);
        }
        // A report of several answers fails with each refused one.
        let answers = document["answers"].as_array().into_iter().flatten();
        let refused: Vec<_> = answers
            .filter_map(|answer| {
                let hint = answer["hint"].as_str().map(str::to_owned);
                let code = answer["code"]
                    .as_str()
                    .filter(|_| answer["$type"] == "board/error/v1")?;
                Some((code.to_owned(), unless_board(code, hint)))
            })
            .collect();
        if !refused.is_empty() {
            return (refused, false);
        }
        let entries = document["entries"].as_array().into_iter().flatten();
        let entries = entries.filter_map(|entry| {
            let hint = entry["hint"].as_str().map(str::to_owned);
            Some((entry["code"].as_str()?.to_owned(), hint))
        });
        return (entries.collect(), true);
    }
    let stderr = text(&output.stderr);
    if let Some(code) = stderr
        .lines()
        .find_map(|line| line.strip_prefix("error: "))
        .and_then(|line| line.split(": ").next())
    {
        let hint = stderr.lines().find_map(|line| line.strip_prefix("hint: "));
        let hint = unless_board(code, hint.map(str::to_owned));
        return (vec![(code.to_owned(), hint)], false);
    }
    let entries = text(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (code, _) = line.strip_prefix("degraded ")?.split_once(' ')?;
            Some((code.to_owned(), Some(String::new())))
        })
        .collect();
    (entries, false)
}

/// The arguments of each `board` command `hint` names in backticks, and
/// whether it names it after a step Board cannot take ("..., then run").
fn commands(hint: &str) -> Vec<(Vec<String>, bool)> {
    let parts: Vec<&str> = hint.split('`').collect();
    parts
        .iter()
        .enumerate()
        .skip(1)
        .step_by(2)
        .filter(|(_, quoted)| quoted.starts_with("board "))
        .map(|(at, quoted)| {
            let manual = parts[at - 1].ends_with("then run ");
            (words(quoted).split_off(1), manual)
        })
        .collect()
}

/// Follows the hints from each starting command in `starts`, run in
/// `place`, adding the codes they fail with to `codes` and each dead end
/// to `dead`.
fn walk(
    place: &Place<'_>,
    starts: &[Start<'_>],
    codes: &mut BTreeSet<String>,
    dead: &mut Vec<Vec<String>>,
) {
    for (code, line) in starts {
        let intended = Some(code.as_str());
        dead.extend(place.follow(&strings(line), intended, None, 0, codes));
    }
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// `scenario`, given an identity to commit as in its own git config, so
/// that what a put does never depends on the machine's git config.
fn identified(scenario: &Scenario) -> &Scenario {
    scenario.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    )
}

/// Every code the property provokes, against `Code::ALL`.
fn unprovoked(codes: &BTreeSet<String>) -> Vec<&'static str> {
    Code::ALL
        .iter()
        .map(|code| code.as_str())
        .filter(|code| !codes.contains(*code))
        .collect()
}

#[test]
fn every_hint_leads_within_three_hops_to_success_or_a_hint_that_names_no_command() {
    let mut codes = BTreeSet::new();
    let mut dead = Vec::new();
    on_the_fixture(&mut codes, &mut dead);
    unopened(&mut codes, &mut dead);
    calling_plugins(&mut codes, &mut dead);
    enabling_plugins(&mut codes, &mut dead);
    apart(&mut codes, &mut dead);
    assert!(dead.is_empty(), "dead ends: {dead:#?}");
    let unprovokable = NOT_REPRODUCED.map(|(code, _)| code.as_str());
    assert_eq!(unprovoked(&codes), unprovokable, "provoked: {codes:?}");
}

/// The seed board as the property starts from it, with an index cache.
struct Fixture {
    board: Scenario,
    /// The board the seed mounts, which must outlive it.
    _mounted: Scenario,
    cache: tempfile::TempDir,
}

impl Fixture {
    /// The seed board with an identity, without its 16 MiB file, which
    /// `apart` reads instead, and with a symlink, a dangling link and a
    /// mount whose followed ref is gone.
    fn new() -> Self {
        let Seed { board, mounted, .. } = Seed::new();
        let unfollowed = mount_of(mounted.workdir().to_str().unwrap(), ObjectId::Sha1([1; 20]))
            .replace(r#""main""#, r#""gone""#);
        identified(&board)
            .write("json/large.json", Item::Deleted)
            .write("symlinked.json", Item::Symlink("projects"))
            .write(
                "links/gone.json",
                Item::File(r#"{"$type": "board/mount/v1", "path": "/projects/none"}"#),
            )
            .write("mount/unfollowed.json", Item::File(&unfollowed));
        Self {
            board,
            _mounted: mounted,
            cache: tempfile::tempdir().unwrap(),
        }
    }

    fn place(&self) -> Place<'_> {
        Place {
            cache: Some(self.cache.path()),
            ..Place::new(self.board.workdir())
        }
    }
}

/// Errors on the seed board: its degraded entries, missing paths, bad
/// queries and usage, each on one shared board, except those whose hints
/// lead to a command that changes the board, which each have their own.
fn on_the_fixture(codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let shared = Fixture::new();
    let fixture = shared.place().shared();
    let report = json(&fixture.run(&strings(&["check", "--json", "--all"])));
    let entries = report["entries"].as_array().unwrap().iter();
    let degraded: Vec<(&str, &str)> = entries
        .filter_map(|entry| Some((entry["logical"].as_str()?, entry["code"].as_str()?)))
        .collect();
    for (path, code) in degraded {
        // Following the hints for these runs `mount update`.
        let own = (path == "/mount/commit").then(Fixture::new);
        let place = own.as_ref().map_or_else(|| fixture.clone(), Fixture::place);
        dead.extend(place.follow(&strings(&["check", path]), Some(code), None, 0, codes));
        // `get`, `ls` and `refs` of a degraded entry may succeed.
        let lines: [&[&str]; 3] = [&["get", path, "--json"], &["ls", path], &["refs", path]];
        for line in lines {
            dead.extend(place.follow(&strings(line), None, None, 0, codes));
        }
    }
    let unbounded = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c) \
                     SELECT count(*) FROM c";
    let starts: [Start<'_>; 22] = [
        (Code::PathNotFound, &["get", "/nowhere"]),
        (Code::PathNotFound, &["get", "/nowhere.json"]),
        (Code::PathNotFound, &["get", "/taks/t1"]),
        (Code::PathNotFound, &["get", "/a/b/c/d"]),
        (Code::PathNotFound, &["ls", "/projects/p/x/y"]),
        (Code::PathNotFound, &["refs", "/taks/t1"]),
        (Code::PathNotFound, &["get", "/projects/p.json"]),
        (Code::RecordNotARecord, &["get", "/projects"]),
        (Code::RefDangling, &["get", "/links/gone"]),
        (Code::RepoUnreadable, &["get", "/mount/m/gone"]),
        (Code::ResolveCycle, &["get", "/cycle/a"]),
        (Code::ResolveTooManyHops, &["get", "/hops/h0"]),
        (Code::QueryInvalid, &["query", "SELEC 1"]),
        (Code::QueryInvalid, &["query", "SELECT * FROM entires"]),
        (Code::QueryInvalid, &["query", "SELECT logcal"]),
        (Code::QueryTimeout, &["query", unbounded]),
        (Code::UsageInvalid, &["chek"]),
        (Code::UsageInvalid, &["get", "projects/p"]),
        (Code::UsageInvalid, &["ls", "--limit", "0"]),
        (Code::UsageInvalid, &["plugin"]),
        (
            Code::MountFollowMissing,
            &["mount", "update", "/mount/unfollowed"],
        ),
        (Code::PathNotFound, &["mount", "update", "/projects/zz"]),
    ];
    walk(&fixture, &starts, codes, dead);
    refused_puts(&fixture, codes, dead);
    // Following these hints runs `mount update` or a put that succeeds.
    let unknown = "1".repeat(40);
    let project = br#"{"$type": "lr/project/v1"}"#;
    let writing: [Start<'_>; 4] = [
        (Code::MountCommitMissing, &["get", "/mount/commit/x"]),
        (Code::PutThroughMount, &["put", "/mount/m/new"]),
        (Code::PutStale, &["put", "/projects/p", "--expect", "none"]),
        (
            Code::PutStale,
            &["put", "/projects/q", "--expect", &unknown],
        ),
    ];
    for start in &writing {
        let own = Fixture::new();
        walk(&own.place().with(project), &[*start], codes, dead);
    }
}

/// Puts the seed board refuses, none of whose hints writes.
fn refused_puts(fixture: &Place<'_>, codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let task = |fields: &str| format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#);
    let project = || r#"{"$type": "lr/project/v1"}"#.to_owned();
    let puts: [(Code, &[&str], String); 13] = [
        (Code::PathCaseCollision, &["put", "/projects/P"], project()),
        (Code::ResolveCycle, &["put", "/cycle/a/new"], project()),
        (Code::RefDangling, &["put", "/links/gone/new"], project()),
        (
            Code::PutInvalid,
            &["put", "/projects/new"],
            r#"{"name": "no type"}"#.into(),
        ),
        (Code::JsonParse, &["put", "/projects/new"], String::new()),
        (Code::JsonParse, &["put", "/projects/new"], "{".into()),
        (
            Code::RecordUnknownType,
            &["put", "/projects/new"],
            r#"{"$type": "lr/note/v1"}"#.into(),
        ),
        (
            Code::SchemaInvalid,
            &["put", "/projects/new"],
            r#"{"$type": "lr/broken/v1"}"#.into(),
        ),
        (Code::PathMalformed, &["put", "/.git/x"], project()),
        (Code::PutSymlink, &["put", "/symlinked"], project()),
        (
            Code::RecordInvalid,
            &["put", "/tasks/t"],
            r#"{"$type": "lr/task/v1", "title": 5}"#.into(),
        ),
        (
            Code::RefMalformed,
            &["put", "/tasks/t"],
            task(r#", "project": "projects/p""#),
        ),
        (
            Code::PutInvalid,
            &["put", "--batch"],
            format!("{}\n{{\n", task("")),
        ),
    ];
    for (code, line, stdin) in &puts {
        walk(
            &fixture.with(stdin.as_bytes()),
            &[(*code, line)],
            codes,
            dead,
        );
    }
}

/// Directories Board cannot open a board in, a new one for each command,
/// since following `check`'s hint runs `init`.
fn unopened(codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let plain = || {
        let board = Scenario::new();
        identified(&board);
        board
    };
    let misconfigured = || {
        let board = Scenario::new();
        let config = Item::File(r#"{"$type": "board/config/v1", "x": 1}"#);
        identified(&board).write(".board/board.json", config);
        board
    };
    // `init` cannot write a file where a directory is.
    let blocked = || {
        let board = Scenario::new();
        identified(&board).write(".board/board.json/x", Item::File("x"));
        board
    };
    let starts: [(fn() -> Scenario, Start<'_>); 5] = [
        (plain, (Code::ConfigNotFound, &["check"])),
        (misconfigured, (Code::ConfigInvalid, &["check"])),
        (misconfigured, (Code::ConfigInvalid, &["init"])),
        (blocked, (Code::ConfigNotFound, &["check"])),
        (blocked, (Code::PutFailed, &["init"])),
    ];
    for (directory, start) in starts {
        let directory = directory();
        walk(&Place::new(directory.workdir()), &[start], codes, dead);
    }
    for start in [
        (Code::ConfigNotFound, &["check"][..]),
        (Code::ConfigNotFound, &["init"]),
    ] {
        let outside = tempfile::tempdir().unwrap();
        walk(&Place::new(outside.path()), &[start], codes, dead);
    }
}

/// `plugin enable` over a schema the board holds otherwise, and `plugin
/// list` on a board lacking an enabled plugin's schema.
fn enabling_plugins(codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let (board, data) = shipping();
    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "array"}"#),
    );
    let place = Place {
        plugins: Some(data.workdir().join("plugins")),
        ..Place::new(board.workdir())
    };
    let start: Start<'_> = (Code::PluginSchemaDiffers, &["plugin", "enable", "notes"]);
    walk(&place, &[start], codes, dead);
    let (board, data) = lacking_a_schema();
    let place = Place {
        plugins: Some(data.workdir().join("plugins")),
        ..Place::new(board.workdir())
    };
    let start: Start<'_> = (Code::PluginSchemaMissing, &["plugin", "list"]);
    walk(&place, &[start], codes, dead);
}

/// Plugin calls that fail.
fn calling_plugins(codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let (board, data) = installed();
    identified(&board);
    install(&data, "tasks", &answering(RESULT, 0));
    install(&data, "notes", &answering(RESULT, 0));
    install(&data, "bad", &answering("not json", 0));
    install(&data, "fails", &answering("", 3));
    install(
        &data,
        "huge",
        "#!/bin/sh\ncat > /dev/null\nhead -c 16777300 /dev/zero\n",
    );
    data.write("plugins/broken/plugin.json", Item::File("{}"))
        .write(
            "plugins/missing/plugin.json",
            Item::File(&TASKS.replace(r#""tasks""#, r#""missing""#)),
        );
    let plugins = Place {
        plugins: Some(data.workdir().join("plugins")),
        ..Place::new(board.workdir())
    }
    .with(b"{}")
    .shared();
    let starts: [Start<'_>; 3] = [
        (Code::PluginNotEnabled, &["call", "notes.due"]),
        (Code::PluginNotInstalled, &["call", "nowhere.due"]),
        (Code::PluginBadManifest, &["call", "broken.due"]),
    ];
    walk(&plugins, &starts, codes, dead);
    let start: Start<'_> = (Code::PluginBadInput, &["call", "tasks.due"]);
    walk(&plugins.with(b"[]"), &[start], codes, dead);
    board.write(
        ".board/board.json",
        Item::File(
            r#"{"$type": "board/config/v1", "plugins": ["tasks", "notes", "bad", "fails", "huge", "missing"]}"#,
        ),
    );
    let starts: [Start<'_>; 5] = [
        (Code::PluginAmbiguous, &["call", "--capability", "due"]),
        (Code::PluginBadOutput, &["call", "bad.due"]),
        (Code::PluginFailed, &["call", "fails.due"]),
        (Code::PluginTooLarge, &["call", "huge.due"]),
        (Code::PluginNotStarted, &["call", "missing.due"]),
    ];
    walk(&plugins, &starts, codes, dead);
    let cache = unwritable_databases(&board, &data);
    let cached = Place {
        cache: Some(cache.workdir()),
        ..plugins.clone()
    };
    let start: Start<'_> = (Code::PluginNoDatabase, &["call", "notes.due"]);
    walk(&cached, &[start], codes, dead);
    let nested = Place {
        depth: Some("8"),
        ..plugins
    };
    let start: Start<'_> = (Code::PluginTooDeep, &["call", "tasks.due"]);
    walk(&nested, &[start], codes, dead);
}

/// A board apart from the fixture, so that no other command reads its
/// 16 MiB file; and a merge under way, since a held lock is waited on for 5
/// seconds.
fn apart(codes: &mut BTreeSet<String>, dead: &mut Vec<Vec<String>>) {
    let apart = board();
    identified(&apart)
        .write("large.json", Item::File(&" ".repeat(16 * 1024 * 1024 + 1)))
        .write(
            ".git/MERGE_HEAD",
            Item::File(&format!("{}\n", "1".repeat(40))),
        );
    let place = Place::new(apart.workdir())
        .with(br#"{"$type": "lr/project/v1"}"#)
        .shared();
    let starts: [Start<'_>; 2] = [
        (Code::JsonTooLarge, &["get", "/large", "--json"]),
        (Code::PutLocked, &["put", "/projects/l"]),
    ];
    walk(&place, &starts, codes, dead);
}
