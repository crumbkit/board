//! Every error leads somewhere (STANDARDS.md 25). crumbkit-test's recovery
//! check runs the built `board` from about 150 failing command lines and
//! follows each command a hint names, up to three hops; its twin check runs
//! each refusal beside a line one word apart that succeeds.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use clap::CommandFactory;
use crumbkit::form::{ReadFailure, Step};
use crumbkit::{FieldValue, Spelling};
use crumbkit_test::{
    Allow, Examples, Failing, FailureReader, Recoveries, Rule, RuntimeFailing, RuntimePlace, Twin,
    Twins, Unprovoked,
};

use super::cli::words;
use super::help::{binary, guide};
use super::plugins::{RESULT, TASKS, answering, lacking_a_schema, shipping};
use super::*;
use crate::run::Cli;

/// Board's failures as crumbkit reads them: each error document, refused
/// answer and entry with a code, with its string fields, and as steps the
/// `board` commands its hint names in backticks. It records each `board
/// check` a hint says to run after a step Board cannot take ("then run").
#[derive(Default)]
struct BoardReader {
    then_run: RefCell<BTreeSet<String>>,
}

impl FailureReader for BoardReader {
    fn read(&self, out: &str, _err: &str) -> Vec<ReadFailure> {
        let Ok(document) = serde_json::from_str::<Value>(out) else {
            return Vec::new();
        };
        let failures = document["failures"].as_array().into_iter().flatten();
        let rows = document["data"].as_array().into_iter().flatten();
        std::iter::once(&document)
            .chain(failures)
            .chain(rows)
            .filter_map(|value| self.failure(value))
            .collect()
    }
}

impl BoardReader {
    fn failure(&self, value: &Value) -> Option<ReadFailure> {
        let code = value["code"].as_str()?;
        let fields = value["fields"].as_object().into_iter().flatten();
        let fields = fields
            .filter_map(|(name, value)| {
                Some((name.clone(), FieldValue::from_json(value)?.into_owned()))
            })
            .collect();
        let recovery = &value["recovery"];
        let call = recovery["call"].as_str().map(|call| format!("`{call}`"));
        let hint = (recovery["text"].as_str().or(value["hint"].as_str())).or(call.as_deref());
        let parts: Vec<&str> = hint.unwrap_or_default().split('`').collect();
        let then_run = (1..parts.len()).step_by(2).filter(|&at| {
            parts[at - 1].ends_with("then run ") && parts[at].starts_with("board check")
        });
        let then_run = then_run.map(|at| parts[at].to_owned());
        self.then_run.borrow_mut().extend(then_run);
        Some(ReadFailure::new(code.to_owned(), steps(hint)).with_fields(fields))
    }
}

/// The `board` commands `hint` names in backticks, or the hint as advice
/// when it names none. Board cannot know another board's directory or the
/// command a reader meant, so a command whose only placeholders are `<dir>`
/// and `<command>` is the reader's to finish, and no step.
fn steps(hint: Option<&str>) -> Vec<Step> {
    let Some(hint) = hint else {
        return Vec::new();
    };
    let readers = |command: &&str| {
        let names: Vec<&str> = command.split('<').skip(1).collect();
        !names.is_empty()
            && names
                .iter()
                .all(|name| name.starts_with("dir>") || name.starts_with("command>"))
    };
    let runs: Vec<Step> = hint
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|quoted| quoted.starts_with("board ") && !readers(quoted))
        .map(|quoted| Step::Run(quoted.to_owned()))
        .collect();
    if runs.is_empty() {
        vec![Step::Hint(hint.to_owned())]
    } else {
        runs
    }
}

/// Board's `code` as crumbkit's.
fn crumbkit_code(code: Code) -> crumbkit::Code {
    let mut parts = code.as_str().split("::");
    let mut next = || parts.next().unwrap();
    crumbkit::Code::new(next(), next(), next())
}

/// `scenario`, given an identity to commit as in its own git config, so
/// that what a put does never depends on the machine's git config.
fn identified(scenario: &Scenario) -> &Scenario {
    scenario.write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    )
}

/// The starts of chains, or the twins, and the directories they run in,
/// which must outlive the checks. A board in `shared` serves several
/// starts, so no chain may change it.
#[derive(Default)]
struct Starts {
    /// Whether these are twins, so no board is built for a start alone.
    twins: bool,
    failing: Vec<Failing<'static>>,
    pairs: Vec<Twin<'static>>,
    untwinned: Vec<Allow>,
    kept: Vec<Box<dyn std::any::Any>>,
    shared: Vec<PathBuf>,
}

impl Starts {
    fn new(twins: bool) -> Self {
        Self {
            twins,
            ..Self::default()
        }
    }

    fn add(&mut self, place: &RuntimePlace, code: Code, line: &str) {
        if !self.twins {
            let failing = place.line(crumbkit_code(code), words(line));
            self.failing.push(failing.into_failing());
        }
    }

    /// A twin: `line`, refused with `code`, and `admitted`, one word apart.
    fn pair(&mut self, place: &RuntimePlace, code: Code, line: &str, admitted: &str) {
        if self.twins {
            let refused = place.line(crumbkit_code(code), words(line));
            self.pairs.push(refused.twin(words(admitted)));
        }
    }

    /// A twin: `line`, refused with `code`, and the same line without the
    /// place's variable `name`.
    fn variable(&mut self, place: &RuntimePlace, code: Code, line: &str, name: &'static str) {
        if self.twins {
            let refused = place.line(crumbkit_code(code), words(line));
            self.pairs.push(refused.variable_twin(name));
        }
    }

    /// `line`, refused with `code` for a reason no word of it names, which
    /// has no twin.
    fn untwinned(&mut self, place: &RuntimePlace, code: Code, line: &str, reason: &'static str) {
        if self.twins {
            let refused = place.line(crumbkit_code(code), words(line));
            self.untwinned.push(refused.untwinned(reason));
        }
    }

    fn keep(&mut self, scenario: Scenario) -> RuntimePlace {
        let place = RuntimeFailing::at(scenario.workdir());
        self.kept.push(Box::new(scenario));
        place
    }

    fn share(&mut self, scenario: Scenario) -> RuntimePlace {
        self.shared.push(scenario.workdir().to_path_buf());
        self.keep(scenario)
    }

    /// The seed board, as `seed_board` builds it, kept for the checks.
    fn seed(&mut self) -> RuntimePlace {
        let board = self.seed_board();
        self.keep(board)
    }

    /// The seed board, without its 16 MiB file, which `apart` reads
    /// instead, and with an identity, a symlink, a dangling link and a mount
    /// whose followed ref is gone.
    fn seed_board(&mut self) -> Scenario {
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
        self.kept.push(Box::new(mounted));
        board
    }
}

/// Every file and symlink below each shared board, with its bytes or target.
fn snapshot(boards: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = boards.to_vec();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                files.insert(path, target.into_os_string().into_encoded_bytes());
            } else {
                // The seed holds a file of mode 000, known by its length.
                let length = || std::fs::metadata(&path).unwrap().len().to_le_bytes().into();
                let bytes = std::fs::read(&path).unwrap_or_else(|_| length());
                files.insert(path, bytes);
            }
        }
    }
    files
}

/// Errors on the seed board: `check`, `get`, `ls` and `refs` of each of its
/// degraded entries, missing paths, bad queries and usage, and refused puts,
/// on one shared board, except those whose hints lead to a command that
/// changes the board, which each have their own. The shared board's runs
/// start with its index in their cache.
fn on_the_seed(starts: &mut Starts) {
    let board = starts.seed_board();
    let dir = board.workdir().to_path_buf();
    let xdg = Scenario::new();
    let cache = xdg.workdir().join("xdg/cache");
    let fixture = starts.share(board).xdg(xdg.workdir().join("xdg"));
    starts.kept.push(Box::new(xdg));
    degraded(starts, &fixture, &dir, &cache);
    refusals(starts, &fixture);
    refused_puts(starts, &fixture);
    writing(starts, &fixture);
}

/// `check`, `get`, `ls` and `refs` of each degraded entry of the seed, read
/// in `dir`, read through the index in `cache`.
fn degraded(starts: &mut Starts, fixture: &RuntimePlace, dir: &Path, cache: &Path) {
    let run = |words: &[&str]| board_cached(dir, Some(cache), words);
    let report = json(&run(&["check", "--json", "--all"]));
    // A twin for each code: one entry's twin shows what any other's would.
    let mut twinned = BTreeSet::new();
    for entry in report["data"].as_array().unwrap() {
        let (Some(path), Some(code)) = (entry["logical"].as_str(), entry["code"].as_str()) else {
            continue;
        };
        // Following the hints for these runs `mount update`, so each start
        // has a board of its own.
        let place = |starts: &mut Starts| {
            if path == "/mount/commit" && !starts.twins {
                starts.seed()
            } else {
                fixture.clone()
            }
        };
        let own = place(starts);
        let line = format!("check {path}");
        starts.add(&own, code_named(code), &line);
        if twinned.insert(code) {
            starts.pair(&own, code_named(code), &line, "check /projects/p");
        }
        // `get`, `ls` and `refs` of a degraded entry may succeed.
        let commands = if starts.twins { 0 } else { 3 };
        for command in &["get", "ls", "refs"][..commands] {
            let output = run(&[command, path, "--json"]);
            let failures = BoardReader::default().read(&text(&output.stdout), "");
            if let (true, Some(failure)) = (output.code != 0, failures.first()) {
                let own = place(starts);
                starts.add(
                    &own,
                    code_named(failure.code()),
                    &format!("{command} {path}"),
                );
            }
        }
    }
}

/// Missing paths, bad queries and usage on the seed.
fn refusals(starts: &mut Starts, fixture: &RuntimePlace) {
    let unbounded = "query 'WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c) \
                     SELECT count(*) FROM c'";
    let zeroes = "query 'SELECT zeroblob(100000000)'";
    let lines: [(Code, &str); 23] = [
        (Code::PathNotFound, "get /nowhere"),
        (Code::PathNotFound, "get /nowhere.json"),
        (Code::PathNotFound, "get /taks/t1"),
        (Code::PathNotFound, "get /a/b/c/d"),
        (Code::PathNotFound, "ls /projects/p/x/y"),
        (Code::PathNotFound, "refs /taks/t1"),
        (Code::PathNotFound, "get /projects/p.json"),
        (Code::RecordNotARecord, "get /projects"),
        (Code::RefDangling, "get /links/gone"),
        (Code::RepoUnreadable, "get /mount/m/gone"),
        (Code::ResolveCycle, "get /cycle/a"),
        (Code::ResolveTooManyHops, "get /hops/h0"),
        (Code::QueryInvalid, "query 'SELEC 1'"),
        (Code::QueryInvalid, "query 'SELECT * FROM entires'"),
        (Code::QueryInvalid, "query 'SELECT logcal'"),
        (Code::QueryTimeout, unbounded),
        (Code::QueryOverBudget, zeroes),
        (Code::UsageInvalid, "chek"),
        (Code::UsageInvalid, "get projects/p"),
        (Code::UsageInvalid, "ls --limit 0"),
        (Code::UsageInvalid, "plugin"),
        (Code::MountFollowMissing, "mount update /mount/unfollowed"),
        (Code::PathNotFound, "mount update /projects/zz"),
    ];
    for (code, line) in lines {
        starts.add(fixture, code, line);
    }
    let pairs: [(Code, &str, &str); 6] = [
        (Code::UsageInvalid, "ls --limit 0", "ls --limit 1"),
        (Code::PathNotFound, "get /nowhere", "get /projects/p"),
        (Code::RecordNotARecord, "get /projects", "get /projects/p"),
        (Code::QueryInvalid, "query 'SELEC 1'", "query 'SELECT 1'"),
        (Code::QueryTimeout, unbounded, "query 'SELECT 1'"),
        (Code::QueryOverBudget, zeroes, "query 'SELECT 1'"),
    ];
    for (code, line, admitted) in pairs {
        starts.pair(fixture, code, line, admitted);
    }
}

/// Errors whose hints lead to a command that changes the seed, and twins
/// whose admitted lines change it, each on a seed of its own.
fn writing(starts: &mut Starts, fixture: &RuntimePlace) {
    let unknown = format!("--expect {}", "1".repeat(40));
    let project = r#"{"$type": "lr/project/v1"}"#;
    let untyped = fixture.clone().stdin(r#"{"name": "no type"}"#);
    starts.untwinned(&untyped, Code::PutInvalid, "put /projects/p", PUT_INVALID);
    if !starts.twins {
        // Following these hints runs `mount update` or a put that succeeds.
        let writing: [(Code, &str); 4] = [
            (Code::MountCommitMissing, "get /mount/commit/x"),
            (Code::PutThroughMount, "put /mount/m/new"),
            (Code::PutStale, "put /projects/p --expect none"),
            (Code::PutStale, &format!("put /projects/q {unknown}")),
        ];
        for (code, line) in writing {
            let own = starts.seed().stdin(project);
            starts.add(&own, code, line);
        }
        return;
    }
    // The admitted lines write, so these twins have a board of their own.
    let own = starts.seed().stdin(project);
    let pairs: [(Code, &str, &str); 5] = [
        (Code::PathMalformed, "put /.git/x", "put /projects/x"),
        (Code::PutSymlink, "put /symlinked", "put /projects/s"),
        (Code::PutThroughMount, "put /mount/m/new", "put /projects/t"),
        (
            Code::PutStale,
            &format!("put /q {unknown}"),
            "put /q --expect none",
        ),
        (
            Code::MountFollowMissing,
            "mount update /mount/unfollowed",
            "mount update /mount/m",
        ),
    ];
    for (code, line, admitted) in pairs {
        starts.pair(&own, code, line, admitted);
    }
}

/// Puts the seed board refuses, none of whose hints writes.
fn refused_puts(starts: &mut Starts, fixture: &RuntimePlace) {
    let task = |fields: &str| format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#);
    let project = r#"{"$type": "lr/project/v1"}"#;
    let new = "put /projects/new";
    let puts: [(Code, &str, &str); 13] = [
        (Code::PathCaseCollision, "put /projects/P", project),
        (Code::ResolveCycle, "put /cycle/a/new", project),
        (Code::RefDangling, "put /links/gone/new", project),
        (Code::PutInvalid, new, r#"{"name": "no type"}"#),
        (Code::JsonParse, new, ""),
        (Code::JsonParse, new, "{"),
        (Code::RecordUnknownType, new, r#"{"$type": "lr/note/v1"}"#),
        (Code::SchemaInvalid, new, r#"{"$type": "lr/broken/v1"}"#),
        (Code::PathMalformed, "put /.git/x", project),
        (Code::PutSymlink, "put /symlinked", project),
        (
            Code::RecordInvalid,
            "put /tasks/t",
            r#"{"$type": "lr/task/v1", "title": 5}"#,
        ),
        (
            Code::RefMalformed,
            "put /tasks/t",
            &task(r#", "project": "projects/p""#),
        ),
        (
            Code::PutInvalid,
            "put --batch",
            &format!("{}\n{{\n", task("")),
        ),
    ];
    for (code, line, stdin) in puts {
        starts.add(&fixture.clone().stdin(stdin), code, line);
    }
}

/// Directories Board cannot open a board in, a new one for each command,
/// since following `check`'s hint runs `init`.
fn unopened(starts: &mut Starts) {
    let misconfigured = (
        ".board/board.json",
        r#"{"$type": "board/config/v1", "x": 1}"#,
    );
    // `init` cannot write a file where a directory is.
    let blocked = (".board/board.json/x", "x");
    // Each line's directory file, if any.
    let lines = [
        (None, Code::ConfigNotFound, "check"),
        (Some(misconfigured), Code::ConfigInvalid, "check"),
        (Some(misconfigured), Code::ConfigInvalid, "init"),
        (Some(blocked), Code::ConfigNotFound, "check"),
        (Some(blocked), Code::PutFailed, "init"),
    ];
    for (file, code, line) in lines {
        if starts.twins && code != Code::PutFailed {
            continue;
        }
        let board = Scenario::new();
        if let Some((path, contents)) = file {
            identified(&board).write(path, Item::File(contents));
        }
        let place = starts.keep(board);
        starts.add(&place, code, line);
        if code == Code::PutFailed {
            starts.untwinned(&place, code, line, PUT_FAILED);
        }
    }
    let outside = tempfile::tempdir().unwrap();
    let place = RuntimeFailing::at(outside.path());
    for line in ["check", "init"] {
        starts.add(&place, Code::ConfigNotFound, line);
    }
    // `-C` names the directory, which is the reason.
    let board = enabling("");
    let line = |dir: &str| format!("-C {dir} check");
    let admitted = line(board.workdir().to_str().unwrap());
    let refused = line(outside.path().to_str().unwrap());
    starts.pair(&place, Code::ConfigNotFound, &refused, &admitted);
    starts.kept.push(Box::new(board));
    starts.kept.push(Box::new(outside));
}

/// An XDG fixture with plugins installed whose calls fail in each way:
/// each offers `due`, as tasks does, and strict's needs a `before`.
fn calling() -> Scenario {
    let manifest = |name: &str| {
        let manifest = TASKS.replace(r#""tasks""#, &format!("{name:?}"));
        let object = r#""inputSchema": {"type": "object"}"#;
        let strict = r#""inputSchema": {"type": "object", "required": ["before"]}"#;
        manifest.replace(object, if name == "strict" { strict } else { object })
    };
    let huge = "#!/bin/sh\ncat > /dev/null\nhead -c 16777300 /dev/zero\n";
    let runs = [
        ("tasks", answering(RESULT, 0)),
        ("notes", answering(RESULT, 0)),
        ("strict", answering(RESULT, 0)),
        ("bad", answering("not json", 0)),
        ("fails", answering("", 3)),
        ("huge", huge.to_owned()),
    ];
    let xdg = Scenario::new();
    let plugins = "xdg/data/board/plugins";
    for (name, run) in runs {
        xdg.write(
            &format!("{plugins}/{name}/plugin.json"),
            Item::File(&manifest(name)),
        )
        .write(&format!("{plugins}/{name}/run"), Item::Executable(&run));
    }
    xdg.write(&format!("{plugins}/broken/plugin.json"), Item::File("{}"))
        .write(
            &format!("{plugins}/missing/plugin.json"),
            Item::File(&manifest("missing")),
        );
    xdg
}

/// A board enabling `plugins`, which commits as whom it says.
fn enabling(plugins: &str) -> Scenario {
    let board = Scenario::new();
    let config = format!(r#"{{"$type": "board/config/v1", "plugins": [{plugins}]}}"#);
    identified(&board).write(".board/board.json", Item::File(&config));
    board
}

/// A board enabling tasks, run with `xdg` as its XDG fixture.
fn tasks(starts: &mut Starts, xdg: &Scenario) -> RuntimePlace {
    starts
        .share(enabling(r#""tasks""#))
        .stdin("{}")
        .xdg(xdg.workdir().join("xdg"))
}

/// Plugin calls that fail.
fn calling_plugins(starts: &mut Starts) {
    let xdg = calling();
    let tasks = tasks(starts, &xdg);
    let lines: [(Code, &str); 3] = [
        (Code::PluginNotEnabled, "call notes.due"),
        (Code::PluginNotInstalled, "call nowhere.due"),
        (Code::PluginBadManifest, "call broken.due"),
    ];
    for (code, line) in lines {
        starts.add(&tasks, code, line);
        starts.pair(&tasks, code, line, "call tasks.due");
    }
    starts.add(
        &tasks.clone().stdin("[]"),
        Code::PluginBadInput,
        "call tasks.due",
    );
    let deep = tasks.clone().env("BOARD_CALL_DEPTH", "8");
    starts.add(&deep, Code::PluginTooDeep, "call tasks.due");
    starts.variable(
        &deep,
        Code::PluginTooDeep,
        "call tasks.due",
        "BOARD_CALL_DEPTH",
    );
    let names = r#""tasks", "notes", "strict", "bad", "fails", "huge", "missing""#;
    let all = starts
        .share(enabling(names))
        .stdin("{}")
        .xdg(xdg.workdir().join("xdg"));
    starts.kept.push(Box::new(xdg));
    // `--capability=due` is one word, so its twin can name a plugin.
    let lines: [(Code, &str, &str); 6] = [
        (
            Code::PluginAmbiguous,
            "call --capability due",
            "call --capability=due",
        ),
        (Code::PluginBadOutput, "call bad.due", "call bad.due"),
        (Code::PluginFailed, "call fails.due", "call fails.due"),
        (Code::PluginTooLarge, "call huge.due", "call huge.due"),
        (
            Code::PluginNotStarted,
            "call missing.due",
            "call missing.due",
        ),
        (Code::PluginBadInput, "", "call strict.due"),
    ];
    for (code, line, twin) in lines {
        if !line.is_empty() {
            starts.add(&all, code, line);
        }
        starts.pair(&all, code, twin, "call tasks.due");
    }
}

/// A plugin call whose database has nowhere to be: the cache directory is a
/// file, and `TMPDIR` names no directory.
fn unkept(starts: &mut Starts) {
    let xdg = calling();
    xdg.write("xdg/cache/board", Item::File(""));
    let place = starts
        .keep(enabling(r#""notes""#))
        .stdin("{}")
        .xdg(xdg.workdir().join("xdg"));
    let place = place.env("TMPDIR", "/nonexistent");
    starts.kept.push(Box::new(xdg));
    let line = "call notes.due";
    starts.add(&place, Code::PluginNoDatabase, line);
    starts.variable(&place, Code::PluginNoDatabase, line, "TMPDIR");
}

/// `plugin enable` over a schema the board holds otherwise, and `plugin
/// list` on a board lacking an enabled plugin's schema.
fn enabling_plugins(starts: &mut Starts) {
    let (board, data) = shipping();
    let xdg = Scenario::new();
    for file in ["plugin.json", "types/notes/note/v1.schema.json"] {
        let text = std::fs::read_to_string(data.workdir().join("plugins/notes").join(file));
        xdg.write(
            &format!("xdg/data/board/plugins/notes/{file}"),
            Item::File(&text.unwrap()),
        );
    }
    xdg.write(
        "xdg/data/board/plugins/tasks/plugin.json",
        Item::File(TASKS),
    );
    board.write(
        ".board/types/notes/note/v1.schema.json",
        Item::File(r#"{"type": "array"}"#),
    );
    let place = starts.keep(board).xdg(xdg.workdir().join("xdg"));
    let line = "plugin enable notes";
    starts.add(&place, Code::PluginSchemaDiffers, line);
    starts.pair(
        &place,
        Code::PluginSchemaDiffers,
        line,
        "plugin enable tasks",
    );
    let place = starts
        .keep(lacking_a_schema().0)
        .xdg(xdg.workdir().join("xdg"));
    starts.kept.push(Box::new(xdg));
    starts.add(&place, Code::PluginSchemaMissing, "plugin list");
    let line = "plugin list";
    starts.untwinned(&place, Code::PluginSchemaMissing, line, SCHEMA_MISSING);
}

/// A board apart from the seed, so that no other command reads its 16 MiB
/// file; and a merge under way, since a held lock is waited on for 5
/// seconds.
fn apart(starts: &mut Starts) {
    let apart = board();
    identified(&apart)
        .write("large.json", Item::File(&" ".repeat(16 * 1024 * 1024 + 1)))
        .write(
            ".git/MERGE_HEAD",
            Item::File(&format!("{}\n", "1".repeat(40))),
        );
    let place = starts.share(apart).stdin(r#"{"$type": "lr/project/v1"}"#);
    starts.add(&place, Code::JsonTooLarge, "get /large --json");
    starts.add(&place, Code::PutLocked, "put /projects/l");
    starts.pair(&place, Code::JsonTooLarge, "get /large", "get /projects/p");
    starts.untwinned(&place, Code::PutLocked, "put /projects/p", PUT_LOCKED);
}

/// Puts through a mount of a board that the mounted repository refuses;
/// following their hints runs `mount update`.
fn through_mounts(starts: &mut Starts) {
    let files = [
        (".board/board.json", CONFIG),
        (
            ".board/types/lr/project/v1.schema.json",
            r#"{"type": "object"}"#,
        ),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
    ];
    // A pin ahead of the mounted HEAD, or a record changed since the pin.
    for (code, reason) in [
        (Code::MountNotAncestor, NOT_ANCESTOR),
        (Code::MountStale, MOUNT_STALE),
    ] {
        let mounted = Scenario::new();
        let first = mounted.commit(&files[..2]);
        for (at, text) in files {
            mounted.write(at, Item::File(text));
        }
        let pinned = mounted.commit(&files);
        identified(mounted.track(pinned));
        let root = board();
        identified(&root).write(
            "vendor.json",
            Item::File(&mount_of(&mounted.path(), pinned)),
        );
        if code == Code::MountNotAncestor {
            mounted.write(".git/refs/heads/main", Item::File(&format!("{first}\n")));
        } else {
            let changed = r#"{"$type": "lr/project/v1", "x": 1}"#;
            mounted.write("projects/p.json", Item::File(changed));
        }
        starts.kept.push(Box::new(mounted));
        let place = starts.keep(root).stdin(r#"{"$type": "lr/project/v1"}"#);
        starts.add(&place, code, "put /vendor/projects/p");
        starts.untwinned(&place, code, "put /vendor/projects/p", reason);
    }
}

/// Every start, or every twin, whose code `ENV` provokes.
fn starts(twins: bool) -> Starts {
    let mut starts = Starts::new(twins);
    on_the_seed(&mut starts);
    unopened(&mut starts);
    calling_plugins(&mut starts);
    unkept(&mut starts);
    enabling_plugins(&mut starts);
    apart(&mut starts);
    through_mounts(&mut starts);
    starts
}

/// The golden reports of the chains.
const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tests/recoveries");

/// The environment of every run.
const ENV: &[(&str, &str)] = &[("PATH", "/usr/bin:/bin")];

const ALLOW: &[Allow] = &[
    Allow::new(
        Rule::SameCode,
        "check",
        "check reports what is still degraded (STANDARDS.md 25): a hint sends a reader to check for the detail, and check repeats the code beside the hint that names the file to fix",
    ),
    Allow::new(
        Rule::NextStep,
        "plugin list",
        "plugin list reports each plugin's problem as its status, beside the message or the schema path that says what is wrong, and gives a listing's entries no hint",
    ),
];

/// Why each code without a twin has none.
const PUT_FAILED: &str = "init fails because a directory stands where board.json goes, and no word of the line names that; init_refuses_a_directory_where_board_json_goes_and_admits_a_file in board-ops/src/init/tests.rs pairs the code";
const PUT_INVALID: &str = "the record on stdin has no $type, and no word of the line names the record; input_that_is_not_one_object_with_a_type_is_refused_and_its_twin_is_admitted in board-ops/src/put/tests.rs pairs the code";
const PUT_LOCKED: &str = "a merge is under way in the repository, and no word of the line names that; a_commit_during_a_merge_or_rebase_is_refused_and_a_clean_one_is_admitted in board-tree/src/tests/commits.rs pairs the code";
const NOT_ANCESTOR: &str = "the mounted repository's HEAD does not descend from the pin, and no word of the line names that; a_mounted_head_that_does_not_descend_from_the_pin_is_refused_and_one_that_does_is_admitted in board-ops/src/put/tests/through.rs pairs the code";
const MOUNT_STALE: &str = "the record changed in the mounted repository since the pin, and no word of the line names that; a_record_changed_in_the_mounted_repository_since_the_pin_is_refused_and_an_unchanged_one_is_admitted in board-ops/src/put/tests/through.rs pairs the code";
const SCHEMA_MISSING: &str = "the board lacks an enabled plugin's schema, and no word of the line names the board's files; an_enabled_plugin_whose_schema_is_missing_is_degraded_and_one_holding_it_is_not in board-ops/src/plugins/tests.rs pairs the code";

/// Board's codes, as crumbkit's, and why each one that no command line in
/// a test provokes is not provoked.
fn declared() -> (Vec<crumbkit::Code>, Vec<Unprovoked<'static>>) {
    let unprovoked = NOT_REPRODUCED.iter();
    let unprovoked =
        unprovoked.map(|(code, reason)| Unprovoked::from_code(crumbkit_code(*code), reason));
    let codes = Code::ALL.into_iter().map(crumbkit_code).collect();
    (codes, unprovoked.collect())
}

/// Follows the hints from `starts`, which must provoke each declared code
/// that a test can, pins the chains in `GOLDENS`, and returns each `board
/// check` a hint says to run after a fix.
fn recover(starts: &[Failing<'_>]) -> BTreeSet<String> {
    let (binary, reader) = (binary(), BoardReader::default());
    let examples = Examples::new(&binary, "")
        .env(ENV)
        .allow(ALLOW)
        .failure_reader(&reader);
    let (codes, unprovoked) = declared();
    let recoveries = Recoveries::from_codes(examples, Spelling::COLONS, &codes)
        .starts(starts)
        .unprovoked(&unprovoked);
    crumbkit_test::check_recoveries(&Cli::command(), &guide(), &recoveries, GOLDENS);
    reader.then_run.take()
}

#[test]
fn every_hint_leads_within_three_hops_to_success_or_advice_and_changes_no_shared_board() {
    let starts = starts(false);
    let before = snapshot(&starts.shared);
    let then_run = recover(&starts.failing);
    let after = snapshot(&starts.shared);
    let changed: Vec<_> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(
        changed.is_empty(),
        "a chain changed a shared board: {changed:?}"
    );
    // The report indents a chain's step by two spaces for each hop, and
    // crumbkit lists, without running it, a fourth hop or a command line the
    // chain ran already. These assertions read only that layout, so they
    // still check the report if crumbkit rewords why it stopped.
    let report = std::fs::read_to_string(format!("{GOLDENS}/recoveries.txt")).unwrap();
    let mut repeated = 0;
    for chain in report.split("\n\n") {
        let mut lines = chain.lines();
        let mut ran: Vec<&str> = lines.next().into_iter().collect();
        for line in lines {
            let step = line.trim_start();
            let Some(command) = step.strip_prefix("run ") else {
                continue;
            };
            let hop = (line.len() - step.len()) / 2;
            assert!(hop <= 3, "a chain is longer than three hops: {line}");
            // A chain may come round only to a `check` that a hint says to
            // run after a step Board cannot take.
            if ran.contains(&command) {
                repeated += 1;
                assert!(
                    then_run.contains(command),
                    "a chain comes round to {command}, which no hint says to run after a fix"
                );
            }
            ran.push(command);
        }
    }
    assert!(
        repeated > 0,
        "no chain comes round to a check, so the rule above checks nothing"
    );
}

#[test]
fn every_refusal_has_a_twin_one_word_apart_that_succeeds() {
    let starts = starts(true);
    let (binary, reader) = (binary(), BoardReader::default());
    let examples = Examples::new(&binary, "")
        .env(ENV)
        .allow(&starts.untwinned)
        .failure_reader(&reader);
    let (codes, unprovoked) = declared();
    let twins = Twins::from_codes(examples, Spelling::COLONS, &codes)
        .pairs(&starts.pairs)
        .unprovoked(&unprovoked);
    crumbkit_test::check_twins(&Cli::command(), &guide(), &twins);
}
