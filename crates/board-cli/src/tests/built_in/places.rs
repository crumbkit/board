//! The places plugin through the binary: `attach`, `detach` and `list` with
//! a stub `mxv` first on the PATH the plugin's processes see, and
//! `BOARD_MACHINE` set, since CI runs on Linux without `scutil`. The real
//! mxv is never on that PATH.

use super::*;

const JOIN: &str = "/places/quokka/fix-login/changes/fix-login";
const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// mxv 0.1.1's refusal of `--fields name,id,path,problem`.
const OLD: &str = r#"{"schema":"mxv/failure/v1","code":"mxv::cli::usage","message":"--fields names an unknown field 'id'","fields":{"argument":"--fields","value":"id"}}"#;

/// A board that enables changes and places and holds the change
/// `/changes/fix-login`, with a directory first on its plugins' `PATH`
/// that holds a stub `mxv` once a test writes one, and a workspace
/// directory for the stub to list.
struct Places {
    board: Fixture,
    bin: Scenario,
    workspaces: Scenario,
}

fn board() -> Places {
    let mut board = Fixture::enabling(&["changes", "places"]);
    board.write(
        "changes/fix-login.json",
        &json!({"$type": "changes/change/v2", "title": "Fix the login", "phase": "build"})
            .to_string(),
    );
    let bin = Scenario::new();
    board.path = format!("{}:{PATH}", bin.workdir().display());
    let workspaces = Scenario::new();
    workspaces.write("fix-login/board/.keep", Item::File(""));
    Places {
        board,
        bin,
        workspaces,
    }
}

/// An `mxv/list/v1` document of `rows`, each a name, a path and an id.
fn listing(rows: &[(&str, &str, Option<&str>)]) -> String {
    let data: Vec<Value> = (rows.iter())
        .map(|(name, path, id)| json!({"name": name, "id": id, "path": path}))
        .collect();
    json!({"schema": "mxv/list/v1", "data": data, "total": data.len(), "truncated": false})
        .to_string()
}

impl Places {
    /// The workspace `fix-login`'s directory.
    fn root(&self) -> String {
        self.workspaces
            .workdir()
            .join("fix-login")
            .display()
            .to_string()
    }

    /// Puts a stub `mxv` on the plugin's `PATH` that logs its arguments and
    /// prints `live` for `mxv list` and `archived` for `mxv list --archived`.
    fn mxv(&self, live: &str, archived: &str) {
        let log = self.bin.workdir().join("mxv.log");
        let stub = format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\ncase \"$2\" in\n--archived) echo '{archived}' ;;\n*) echo '{live}' ;;\nesac\n",
            log.display()
        );
        self.bin.write("mxv", Item::Executable(&stub));
    }

    /// A stub `mxv` that lists the workspace `fix-login` with `id`.
    fn listing(&self, id: Option<&str>) {
        let root = self.root();
        self.mxv(&listing(&[("fix-login", &root, id)]), &listing(&[]));
    }

    /// Each argument line the stub `mxv` was run with.
    fn mxv_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.bin.workdir().join("mxv.log"))
            .map(|log| log.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Runs `board --json <arguments>` started in `caller`, with
    /// `BOARD_MACHINE` set, keeping its index in the cache unless `cold`,
    /// and returns its exit code and document.
    fn run(&self, arguments: &[&str], caller: Option<&Path>, cold: bool) -> (u8, Value) {
        let context = Context {
            start: self.board.board.workdir().to_path_buf(),
            home: None,
            cache: (!cold).then(|| self.board.cache.path().to_path_buf()),
            plugins: None,
            boards: None,
            signer: None,
            allowed_signers: None,
            attention_allowed_signers: None,
            environment: board_plugin::Environment {
                path: Some(self.board.path.clone().into()),
                caller: caller.map(|caller| caller.as_os_str().to_owned()),
                inherited: [(OsString::from("BOARD_MACHINE"), OsString::from("quokka"))]
                    .into_iter()
                    .collect(),
                ..board_plugin::Environment::default()
            },
            complete: None,
            terminal: false,
            browser: None,
            binary: None,
            cursor: None,
            built_in: board_plugins::BUILT_IN,
        };
        let arguments = [BINARY_NAME, "--json"]
            .into_iter()
            .chain(arguments.iter().copied())
            .map(OsString::from);
        let mut ran = Ran::default();
        let output = run(arguments, &context, &mut &b""[..], &mut ran);
        (output.code, json(&output))
    }

    /// The output of `board call places.<capability>` with `flags`, which
    /// must answer.
    fn call(&self, capability: &str, flags: &[&str]) -> Value {
        self.call_from(None, capability, flags)
    }

    fn call_from(&self, caller: Option<&Path>, capability: &str, flags: &[&str]) -> Value {
        let target = format!("places.{capability}");
        let arguments: Vec<&str> = ["call", target.as_str()]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let (code, report) = self.run(&arguments, caller, false);
        assert_eq!(code, 0, "{report}");
        report["data"]["output"].clone()
    }

    /// The exit code and failure of `board call places.<capability>`.
    fn refused(&self, capability: &str, flags: &[&str]) -> (u8, Value) {
        let target = format!("places.{capability}");
        let arguments: Vec<&str> = ["call", target.as_str()]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        self.run(&arguments, None, false)
    }
}

const ATTACH: [&str; 4] = ["--change", "/changes/fix-login", "--workspace", "fix-login"];

#[test]
fn attach_writes_a_join_board_checks_whole_and_refs_finds_from_the_change_and_the_workspace() {
    let board = board();
    board.listing(Some(ID_A));
    let output = board.call("attach", &ATTACH);
    assert_eq!(
        output,
        json!({
            "path": JOIN,
            "machine": "quokka",
            "workspace": "fix-login",
            "change": "/changes/fix-login",
            "workspace_id": ID_A,
            "written": true,
            "replaced": false,
        })
    );
    assert_eq!(
        board.mxv_calls(),
        ["list --all --json --fields name,id,path,problem"]
    );
    assert_eq!(board.board.checked("/places/"), codes(&[(JOIN, None)]));
    for target in ["/changes/fix-login", "/places/quokka/fix-login"] {
        let (code, refs) = board.run(&["refs", target], None, false);
        assert_eq!(code, 0, "{refs}");
        assert!(refs.to_string().contains(JOIN), "{target}: {refs}");
    }
}

#[test]
fn attach_from_inside_the_workspace_needs_no_name_and_from_outside_it_is_refused() {
    let board = board();
    board.listing(Some(ID_A));
    let outside = board.workspaces.workdir().to_path_buf();
    let (code, refused) = board.run(
        &["call", "places.attach", "--change", "/changes/fix-login"],
        Some(&outside),
        false,
    );
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("places::workspace::outside")),
        "{refused}"
    );
    let inside = board.workspaces.workdir().join("fix-login/board");
    let output = board.call_from(Some(&inside), "attach", &["--change", "/changes/fix-login"]);
    assert_eq!(output["path"], JOIN);
}

#[test]
fn the_places_command_runs_each_capability_as_board_call_does() {
    let board = board();
    board.listing(Some(ID_A));
    let (code, attached) = board.run(
        &[
            "places",
            "attach",
            "--change",
            "/changes/fix-login",
            "--workspace",
            "fix-login",
        ],
        None,
        false,
    );
    assert_eq!(code, 0, "{attached}");
    let (code, detached) = board.run(
        &[
            "places",
            "detach",
            "--change",
            "/changes/fix-login",
            "--workspace",
            "fix-login",
        ],
        None,
        false,
    );
    assert_eq!(code, 0, "{detached}");
    assert!(
        detached.to_string().contains("\"removed\":true"),
        "{detached}"
    );
}

#[test]
fn list_answers_from_a_warm_index_as_from_a_cold_one_after_a_join_is_written_and_removed() {
    let board = board();
    board.listing(Some(ID_A));
    let answer = |cold: bool| {
        let (code, report) = board.run(&["call", "places.list"], None, cold);
        assert_eq!(code, 0, "{report}");
        report["data"]["output"].clone()
    };
    let before = answer(false);
    assert_eq!(before["joins"], json!([]));
    board.call("attach", &ATTACH);
    let warm = answer(false);
    assert_ne!(warm, before, "the join is seen");
    assert_eq!(
        warm["joins"][0],
        json!({
            "path": JOIN,
            "machine": "quokka",
            "workspace": "fix-login",
            "workspace_id": ID_A,
            "since": warm["joins"][0]["since"],
            "change": {"path": "/changes/fix-login", "title": "Fix the login", "phase": "build"},
            "status": "live",
        })
    );
    assert_eq!(warm, answer(true));
    board.call("detach", &ATTACH);
    let removed = answer(false);
    assert_eq!(removed["joins"], json!([]));
    assert_eq!(removed, answer(true));
}

#[test]
fn list_derives_replaced_archived_gone_and_unknown_from_what_mxv_lists() {
    let board = board();
    board.listing(Some(ID_A));
    board.call("attach", &ATTACH);
    let root = board.root();
    let status = |live: &str, archived: &str| {
        board.mxv(live, archived);
        board.call("list", &[])["joins"][0]["status"].clone()
    };
    let workspace = |id: Option<&str>| listing(&[("fix-login", &root, id)]);
    assert_eq!(status(&workspace(Some(ID_B)), &listing(&[])), "replaced");
    assert_eq!(status(&listing(&[]), &workspace(Some(ID_A))), "archived");
    assert_eq!(status(&listing(&[]), &listing(&[])), "gone");
    assert_eq!(status(&workspace(None), &listing(&[])), "unknown");
    assert_eq!(status(&workspace(Some(ID_A)), &listing(&[])), "live");
}

#[test]
fn an_mxv_too_old_to_print_ids_is_refused_with_no_id_and_a_current_one_is_admitted() {
    let board = board();
    let log = board.bin.workdir().join("mxv.log");
    let stub = format!(
        "#!/bin/sh\necho \"$@\" >> \"{}\"\ncat <<'EOF'\n{OLD}\nEOF\ncat >&2 <<'EOF'\n{OLD}\nEOF\nexit 2\n",
        log.display()
    );
    let old = || board.bin.write("mxv", Item::Executable(&stub));
    let refuse = |capability: &str, flags: &[&str]| {
        let (code, refused) = board.refused(capability, flags);
        assert_eq!(
            (code, &refused["code"]),
            (1, &json!("places::mxv::no_id")),
            "{refused}"
        );
        assert!(
            refused["message"].as_str().unwrap().contains("0.4.0"),
            "{refused}"
        );
    };
    old();
    refuse("attach", &ATTACH);
    board.listing(Some(ID_A));
    assert_eq!(board.call("attach", &ATTACH)["written"], true);
    // list asks mxv only once this machine has a join to show.
    old();
    refuse("list", &[]);
}

#[test]
fn attach_to_a_workspace_mxv_prints_with_a_null_id_is_refused_and_its_twin_with_an_id_is_admitted()
{
    let board = board();
    board.listing(None);
    let (code, refused) = board.refused("attach", &ATTACH);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("places::workspace::no_id")),
        "{refused}"
    );
    assert_eq!(board.board.checked("/places/"), codes(&[]));
    board.listing(Some(ID_A));
    assert_eq!(board.call("attach", &ATTACH)["written"], true);
}

#[test]
fn attach_to_a_workspace_mxv_cannot_read_names_mxvs_problem_and_its_readable_twin_is_admitted() {
    let board = board();
    let row = json!({"name": "fix-login", "id": null, "path": board.root(), "problem": "the record does not parse"});
    let live = json!({"schema": "mxv/list/v1", "data": [row], "total": 1, "truncated": false});
    board.mxv(&live.to_string(), &listing(&[]));
    let (code, refused) = board.refused("attach", &ATTACH);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("places::workspace::no_id")),
        "{refused}"
    );
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("the record does not parse"), "{message}");
    board.listing(Some(ID_A));
    assert_eq!(board.call("attach", &ATTACH)["written"], true);
}

#[test]
fn without_mxv_on_path_attach_is_failed_and_detach_with_a_workspace_runs_none() {
    let board = board();
    let (code, refused) = board.refused("attach", &ATTACH);
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("places::mxv::failed")),
        "{refused}"
    );
    board.board.write(
        "places/quokka/fix-login/changes/fix-login.json",
        &json!({
            "$type": "places/workspace-change/v1",
            "workspace": "/places/quokka/fix-login",
            "change": "/changes/fix-login",
            "workspace_id": ID_A,
            "since": "2026-11-20T09:00:00Z",
        })
        .to_string(),
    );
    assert_eq!(
        board.call("detach", &ATTACH),
        json!({"path": JOIN, "removed": true})
    );
}

#[test]
fn a_change_path_with_nothing_there_is_refused_and_its_twin_is_admitted() {
    let board = board();
    board.listing(Some(ID_A));
    let (code, refused) = board.refused(
        "attach",
        &["--change", "/changes/fix-logon", "--workspace", "fix-login"],
    );
    assert_eq!(
        (code, &refused["code"]),
        (1, &json!("places::change::not_found")),
        "{refused}"
    );
    assert_eq!(board.call("attach", &ATTACH)["written"], true);
}

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schema_it_ships() {
    let board = board();
    let report = board.board.json(&["plugin", "list"]);
    let listed = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "places")
        .unwrap();
    assert_eq!(
        (&listed["status"], &listed["enabled"], &listed["offers"]),
        (
            &json!("ok"),
            &json!(true),
            &json!(["attach", "detach", "list"])
        )
    );
    let shipped = shipped("places");
    assert_eq!(shipped.len(), 1, "the join schema");
    for (path, text) in shipped {
        assert_eq!(board.board.read(&path), text, "{path}");
    }
}

#[test]
fn list_prints_every_machines_joins_with_other_machines_unknown_and_machine_narrows_it() {
    let board = board();
    board.listing(Some(ID_A));
    board.call("attach", &ATTACH);
    board.board.write(
        "places/wombat/elsewhere/changes/fix-login.json",
        &json!({
            "$type": "places/workspace-change/v1",
            "workspace": "/places/wombat/elsewhere",
            "change": "/changes/fix-login",
            "workspace_id": ID_B,
            "since": "2026-11-20T09:00:00Z",
        })
        .to_string(),
    );
    let every = board.call("list", &[]);
    let rows: Vec<(&str, &str)> = (every["joins"].as_array().unwrap().iter())
        .map(|row| {
            (
                row["machine"].as_str().unwrap(),
                row["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, [("quokka", "live"), ("wombat", "unknown")]);
    let calls = board.mxv_calls().len();
    let wombat = board.call("list", &["--machine", "wombat"]);
    assert_eq!(wombat["total"], 1);
    assert_eq!(wombat["joins"][0]["workspace"], "elsewhere");
    assert_eq!(
        board.mxv_calls().len(),
        calls,
        "mxv is not asked about wombat"
    );
    assert_eq!(
        board.call("list", &["--machine", "bilby"])["joins"],
        json!([])
    );
    let (code, refused) = board.refused("list", &["--machine", "Wombat"]);
    assert_eq!(code, 1, "{refused}");
    assert_eq!(refused["code"], "board::plugin::bad_input", "{refused}");
}
