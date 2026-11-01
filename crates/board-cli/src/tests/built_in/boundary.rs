//! The boundary plugin through the binary: `boundary.check` evaluates
//! content against the boundaries that match it and records the crossing in
//! the personal board. `overrides.rs` holds the owner's override and
//! `verify.rs` the other two capabilities. These were the plugin's pytest
//! suite.

use board_scenario::signer::Signer;

use super::*;

mod overrides;
mod verify;

/// An expression whose text differs from anything it matches, so a quote
/// of a match never reveals it.
const PRIVATE: &str = r"acme-\d{3}";

/// The owner's personal board, which enables controls and boundary and does
/// not commit, named in a known boards file, with the test signer, whose key
/// the allowed signers file holds.
struct Personal {
    board: Scenario,
    xdg: Scenario,
    signer: Signer,
}

impl Personal {
    fn new() -> Self {
        let personal = Self {
            board: uncommitted(),
            xdg: Scenario::new(),
            signer: Signer::new(),
        };
        let known = json!({"personal": personal.board.path()}).to_string();
        personal.xdg.write("board/config.json", Item::File(&known));
        personal.enable(personal.board.workdir(), &["controls", "boundary"]);
        personal
    }

    /// Enables each of `plugins` in the board at `directory`.
    fn enable(&self, directory: &Path, plugins: &[&str]) {
        for plugin in plugins {
            let enabled = self.run_in(directory, &["plugin", "enable", plugin], "");
            assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        }
    }

    /// Runs `board` in `directory` with the known boards file, the signer
    /// and the allowed signers file, and `stdin` as its standard input.
    fn run_in(&self, directory: &Path, arguments: &[&str], stdin: &str) -> Output {
        let context = Context {
            start: directory.to_path_buf(),
            home: None,
            cache: None,
            plugins: None,
            boards: Some(self.xdg.workdir().join("board/config.json")),
            signer: Some(self.signer.program().into_os_string()),
            allowed_signers: Some(self.signer.allowed()),
            environment: board_plugin::Environment {
                path: Some("/usr/bin:/bin".into()),
                ..board_plugin::Environment::default()
            },
            complete: None,
            terminal: false,
            browser: None,
            binary: None,
            cursor: None,
            built_in: board_plugins::BUILT_IN,
        };
        let arguments = std::iter::once(BINARY_NAME)
            .chain(arguments.iter().copied())
            .map(OsString::from);
        run(
            arguments,
            &context,
            &mut stdin.as_bytes(),
            &mut Ran::default(),
        )
    }

    /// Runs `board` in the personal board's root.
    fn run(&self, arguments: &[&str]) -> Output {
        self.run_in(self.board.workdir(), arguments, "")
    }

    fn write(&self, path: &str, text: &str) -> &Self {
        self.board.write(path, Item::File(text));
        self
    }

    /// The record at `path`, read from the working tree, or `None`.
    fn record(&self, path: &str) -> Option<Value> {
        let file = (self.board.workdir()).join(format!("{}.json", path.trim_start_matches('/')));
        let text = std::fs::read_to_string(file).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }

    /// `board --json call boundary.<capability>` in `directory` with
    /// `input` on stdin, and its one document.
    fn call_in(&self, directory: &Path, capability: &str, input: &Value) -> (Output, Value) {
        let target = format!("boundary.{capability}");
        let ran = self.run_in(directory, &["--json", "call", &target], &input.to_string());
        let document = json(&ran);
        (ran, document)
    }

    fn call(&self, capability: &str, input: &Value) -> (Output, Value) {
        self.call_in(self.board.workdir(), capability, input)
    }

    /// The ids of the pending effects, sorted.
    fn pending(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(self.board.workdir().join("effects/pending")) else {
            return Vec::new();
        };
        let mut ids: Vec<String> = (entries.map(|entry| entry.unwrap().path()))
            .filter_map(|path| Some(path.file_stem()?.to_str()?.to_owned()))
            .collect();
        ids.sort();
        ids
    }

    /// `board answer <effect> <answer>`, as the owner, whose approval the
    /// test signer signs.
    fn answer(&self, effect: &str, answer: &str) {
        let answered = self.run(&["answer", effect, answer]);
        assert_eq!(answered.code, 0, "{}", text(&answered.stderr));
    }
}

/// A board that does not commit.
fn uncommitted() -> Scenario {
    let board = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    board.write(".board/board.json", Item::File(config));
    board
}

/// A `controls/control/v1` record with `rule`.
fn control(rule: &Value) -> String {
    json!({"$type": "controls/control/v1", "requirement": "A test.", "purpose": "A test.", "rule": rule})
        .to_string()
}

/// A `boundary/boundary/v1` record with `fields`.
fn boundary(fields: &Value) -> String {
    record(json!({"$type": "boundary/boundary/v1"}), fields)
}

/// A check of every part against `/controls/names`.
fn names_check() -> Value {
    json!([{"control": "/controls/names", "parts": ["*"]}])
}

/// A pattern control at `/controls/names`, shown match-only, whose file
/// holds [`PRIVATE`].
fn private_names(board: &Personal) {
    board
        .write("private/names.txt", &format!("{PRIVATE}\n"))
        .write(
            "controls/names.json",
            &control(&json!({"pattern": "/private/names.txt", "show": "match-only"})),
        );
}

/// A boundary at `/boundaries/octo` that checks every part of a pull
/// request to octo against `/controls/names`, with `fields` changed.
fn names_boundary(board: &Personal, fields: &Value) {
    let base = json!({
        "$type": "boundary/boundary/v1",
        "to": "github:octo/*",
        "actions": ["github.pr-*"],
        "checks": names_check(),
    });
    board.write("boundaries/octo.json", &record(base, fields));
}

/// A pull request to octo/board from `/repos/board`, with `parts`.
fn pr(parts: &Value) -> Value {
    json!({"action": "github.pr-create", "to": "github:octo/board", "from": "/repos/board", "parts": parts})
}

/// The capability's output in a `--json` report.
fn output(document: &Value) -> &Value {
    assert!(document.get("data").is_some(), "{document}");
    &document["data"]["output"]
}

/// Everything `ran` printed.
fn printed(ran: &Output) -> String {
    text(&ran.stdout) + &text(&ran.stderr)
}

#[test]
fn content_that_no_boundary_covers_passes_unchecked_and_its_crossing_says_so() {
    let personal = Personal::new();
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let result = output(&document);
    assert_eq!(
        (&result["boundaries"], &result["verdicts"]),
        (&json!([]), &json!([]))
    );
    assert_eq!(
        result["note"],
        "no boundary matches github.pr-create to github:octo/board from /repos/board; nothing was checked"
    );
    let crossing = personal
        .record(result["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        (&crossing["passed"], &crossing["boundaries"]),
        (&json!(true), &json!([]))
    );
}

#[test]
fn a_private_match_refuses_naming_control_part_and_line_and_never_the_pattern() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    let parts = json!({"pr.title": "Fine", "pr.body": "Fine.\nSee acme-123 here."});
    let (ran, document) = personal.call("check", &pr(&parts));
    assert_eq!(ran.code, 1, "{}", printed(&ran));
    assert_eq!(document["code"], "boundary::check::failed");
    let fields = &document["fields"];
    assert_eq!(
        fields["failures"],
        json!([{"control": "/controls/names", "part": "pr.body", "text": "acme-123", "line": 2}])
    );
    assert_eq!(fields.get("recovery"), None);
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("Next: change pr.body:"), "{hint}");
    assert!(
        hint.contains(
            "run `board call github.pr-create` again, or `board --personal call boundary.check`"
        ),
        "{hint}"
    );
    let message = document["message"].as_str().unwrap();
    assert!(
        message.contains(r#"/controls/names on pr.body line 2: "acme-123""#),
        "{message}"
    );
    assert!(!message.contains("Next:"), "{message}");
    assert!(!printed(&ran).contains(PRIVATE));
    let crossing = personal
        .record(fields["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(crossing["passed"], false);
}

#[test]
fn the_same_content_without_the_match_passes_and_its_crossing_holds_every_verdict() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    let parts = json!({"pr.title": "Fine", "pr.body": "Fine.\nSee here."});
    let (ran, document) = personal.call("check", &pr(&parts));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let result = output(&document);
    assert_eq!(result["boundaries"], json!(["/boundaries/octo"]));
    assert_eq!(result["note"], Value::Null);
    let statuses: Vec<(&Value, &Value)> = (result["verdicts"].as_array().unwrap().iter())
        .map(|verdict| (&verdict["part"], &verdict["status"]))
        .collect();
    assert_eq!(
        statuses,
        [
            (&json!("pr.body"), &json!("passed")),
            (&json!("pr.title"), &json!("passed"))
        ]
    );
    let crossing = personal
        .record(result["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(crossing["passed"], true);
    assert_eq!(crossing["verdicts"], result["verdicts"]);
    assert_eq!(crossing["from"], "/repos/board");
}

#[test]
fn the_digest_names_the_crossing_and_is_the_sha256_of_the_canonical_json_python_writes() {
    let personal = Personal::new();
    let (_, document) = personal.call("check", &pr(&json!({"pr.body": "Fïne.", "pr.title": "T"})));
    // python3 -c 'import hashlib, json; print(hashlib.sha256(json.dumps({"action": "github.pr-create",
    //   "parts": {"pr.body": "Fïne.", "pr.title": "T"}, "to": "github:octo/board"}, sort_keys=True,
    //   separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())'
    let expected = "aa46b4f4d17f4e7c8796d1cea265d404e5fc11c9ecee18e13dae54dc5cb8b54c";
    assert_eq!(output(&document)["digest"], expected);
    assert_eq!(
        output(&document)["crossing"],
        format!("/crossings/{expected}")
    );
}

#[test]
fn a_check_applies_only_to_the_parts_its_globs_name() {
    let personal = Personal::new();
    personal.write(
        "controls/short.json",
        &control(&json!({"budget": "words", "max": 2})),
    );
    let checks = json!([{"control": "/controls/short", "parts": ["pr.title"]}]);
    personal.write(
        "boundaries/octo.json",
        &boundary(&json!({"to": "github:octo/*", "actions": ["*"], "checks": checks})),
    );
    let (ran, document) = personal.call(
        "check",
        &pr(&json!({"pr.title": "Two words", "pr.body": "Many more words."})),
    );
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let parts: Vec<&Value> = (output(&document)["verdicts"].as_array().unwrap().iter())
        .map(|verdict| &verdict["part"])
        .collect();
    assert_eq!(parts, [&json!("pr.title")]);
    assert_eq!(output(&document)["unchecked"], json!(["pr.body"]));
    let (_, document) = personal.call("check", &pr(&json!({"pr.body": "Many more words."})));
    assert_eq!(output(&document)["boundaries"], json!(["/boundaries/octo"]));
    assert_eq!(
        output(&document)["note"],
        "the boundaries that match name no control for these parts; nothing was checked"
    );
    let (ran, document) = personal.call("check", &pr(&json!({"pr.title": "Three words here"})));
    assert_eq!(ran.code, 1);
    assert_eq!(
        document["fields"]["failures"],
        json!([{"control": "/controls/short", "part": "pr.title", "text": "3 words, more than the 2 allowed"}])
    );
}

#[test]
fn a_boundary_matches_by_destination_action_and_checkout() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({"from": ["/repos/board"]}));
    let parts = json!({"pr.body": "acme-123"});
    let base = pr(&parts);
    let changed = |fields: Value| record(base.clone(), &fields);
    let mut unnamed = base.clone();
    unnamed.as_object_mut().unwrap().remove("from");
    let covered = [
        base.to_string(),
        changed(json!({"from": "/repos/board/docs"})),
        changed(json!({"to": "github:Octo/Board"})),
        unnamed.to_string(),
    ];
    let uncovered = [
        changed(json!({"to": "github:other/board"})),
        changed(json!({"action": "github.push"})),
        changed(json!({"from": "/repos/boarding"})),
    ];
    for given in covered {
        let (_, document) = personal.call("check", &serde_json::from_str(&given).unwrap());
        assert_eq!(document["code"], "boundary::check::failed", "{given}");
    }
    for given in uncovered {
        let (ran, document) = personal.call("check", &serde_json::from_str(&given).unwrap());
        assert_eq!(ran.code, 0, "{given}");
        assert!(output(&document)["note"].is_string(), "{given}");
    }
}

#[test]
fn a_destination_glob_matches_with_sqlite_glob_so_a_set_matches_and_a_negated_set_refuses() {
    let personal = Personal::new();
    private_names(&personal);
    for (to, covered) in [("github:[o]cto/*", true), ("github:[^o]cto/*", false)] {
        names_boundary(&personal, &json!({"to": to}));
        let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "acme-123"})));
        assert_eq!(ran.code != 0, covered, "{to}: {document}");
    }
}

#[test]
fn a_boundary_without_from_covers_a_checkout_no_board_mounts() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    let given = json!({"action": "github.pr-create", "to": "github:octo/board", "parts": {"pr.body": "acme-123"}});
    let (ran, document) = personal.call("check", &given);
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
}

/// A judged control at `/controls/plain` that a boundary to any GitHub
/// destination applies to every pull request body.
fn judged(personal: &Personal) {
    personal
        .write("rubrics/plain.md", "Write plainly.\n")
        .write(
            "controls/plain.json",
            &control(&json!({"judged": "/rubrics/plain.md"})),
        );
    let checks = json!([{"control": "/controls/plain", "parts": ["pr.body"]}]);
    personal.write(
        "boundaries/prose.json",
        &boundary(&json!({"to": "github:*", "actions": ["github.*"], "checks": checks})),
    );
}

#[test]
fn a_judged_rule_is_skipped_for_a_part_another_boundary_fails_on_a_pattern() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    judged(&personal);
    let (_, document) = personal.call("check", &pr(&json!({"pr.body": "Clever. acme-123"})));
    assert_eq!(document["code"], "boundary::check::failed");
    let crossing = personal
        .record(document["fields"]["crossing"].as_str().unwrap())
        .unwrap();
    let verdicts: BTreeMap<&str, &str> = (crossing["verdicts"].as_array().unwrap().iter())
        .map(|verdict| {
            (
                verdict["control"].as_str().unwrap(),
                verdict["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        verdicts,
        BTreeMap::from([
            ("/controls/names", "failed"),
            ("/controls/plain", "skipped")
        ])
    );
}

#[test]
fn a_judged_rule_with_a_part_to_read_refuses_until_board_can_ask_a_model() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    judged(&personal);
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Clever."})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::board::refused");
    assert_eq!(document["fields"]["code"], "controls::judged::unavailable");
    assert_eq!(document["fields"]["control"], "/controls/plain");
}

#[test]
fn an_invalid_boundary_refuses_every_check_naming_it_and_the_same_boundary_made_valid_is_used() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    let broken = json!({"to": "github:*", "action": ["github.*"], "checks": []});
    personal.write("boundaries/broken.json", &boundary(&broken));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::boundary::invalid");
    assert_eq!(
        document["fields"]["boundaries"],
        json!({"/boundaries/broken": "board::record::invalid"})
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(
        hint.contains("run `board --personal check /boundaries/broken`"),
        "{hint}"
    );
    let fixed = json!({"to": "github:*", "actions": ["github.*"], "checks": names_check()});
    personal.write("boundaries/broken.json", &boundary(&fixed));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    assert_eq!(
        output(&document)["boundaries"],
        json!(["/boundaries/broken", "/boundaries/octo"])
    );
}

#[test]
fn a_boundary_file_that_does_not_parse_or_has_another_type_refuses_and_made_valid_is_used() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    for text in ["{", r#"{"$type": "boundary/boundry/v1"}"#] {
        personal.write("boundaries/broken.json", text);
        let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
        assert_eq!(ran.code, 1, "{text}");
        assert_eq!(document["code"], "boundary::boundary::invalid", "{text}");
        let named: Vec<&String> = document["fields"]["boundaries"]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(named, ["/boundaries/broken"], "{text}");
    }
    let fixed = json!({"to": "github:*", "actions": ["github.*"], "checks": names_check()});
    personal.write("boundaries/broken.json", &boundary(&fixed));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    assert_eq!(
        output(&document)["boundaries"],
        json!(["/boundaries/broken", "/boundaries/octo"])
    );
}

#[test]
fn an_entry_under_boundaries_that_is_not_a_boundary_refuses_with_the_step_to_move_it_out() {
    let personal = Personal::new();
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    personal.write("boundaries/README.md", "# Boundaries\n");
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 1);
    assert_eq!(
        document["fields"]["boundaries"],
        json!({"/boundaries/README.md": "not a boundary"})
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(
        hint.contains("move each entry that is not a boundary out of /boundaries/"),
        "{hint}"
    );
    personal.board.write("boundaries/README.md", Item::Deleted);
    personal.write("README.md", "# Boundaries\n");
    let (ran, _) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
}

#[test]
fn a_boundary_outside_boundaries_is_used() {
    let personal = Personal::new();
    private_names(&personal);
    let fields = json!({"to": "github:octo/*", "actions": ["github.*"], "checks": names_check()});
    personal.write("policies/octo.json", &boundary(&fields));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "acme-123"})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    let (_, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(output(&document)["boundaries"], json!(["/policies/octo"]));
}

#[test]
fn a_symlink_at_boundaries_refuses_and_the_same_files_in_a_directory_pass() {
    let personal = Personal::new();
    private_names(&personal);
    let fields = json!({"to": "github:octo/*", "actions": ["github.*"], "checks": names_check()});
    personal.write("policies/octo.json", &boundary(&fields));
    personal
        .board
        .write("boundaries", Item::Symlink("policies"));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::boundary::invalid");
    assert_eq!(
        document["fields"]["boundaries"]["/boundaries"],
        "is not a directory"
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.contains("make /boundaries/ a directory"), "{hint}");
    personal.board.write("boundaries", Item::Deleted);
    personal.board.write("policies/octo.json", Item::Deleted);
    personal.write("boundaries/octo.json", &boundary(&fields));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    assert_eq!(output(&document)["boundaries"], json!(["/boundaries/octo"]));
}

#[test]
fn a_control_without_a_rule_fails_the_crossing_and_the_same_control_with_a_rule_passes() {
    let personal = Personal::new();
    let written =
        json!({"$type": "controls/control/v1", "requirement": "Names.", "purpose": "A test."});
    personal.write("controls/names.json", &written.to_string());
    names_boundary(&personal, &json!({}));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    let failures = document["fields"]["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["control"], "/controls/names");
    let finding = failures[0]["text"].as_str().unwrap();
    assert!(
        finding.starts_with("this control has no rule, so it cannot check content"),
        "{finding}"
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(
        hint.starts_with("Next: give each control without a rule a rule"),
        "{hint}"
    );
    private_names(&personal);
    let (ran, _) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
}

#[test]
fn a_boundary_naming_a_missing_control_refuses_with_the_controls_code() {
    let personal = Personal::new();
    private_names(&personal);
    let gone = json!([{"control": "/controls/gone", "parts": ["*"]}]);
    names_boundary(&personal, &json!({"checks": gone}));
    let (ran, document) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::board::refused");
    assert_eq!(document["fields"]["code"], "controls::control::not_found");
    names_boundary(&personal, &json!({}));
    let (ran, _) = personal.call("check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
}

#[test]
fn a_check_called_in_another_board_records_the_crossing_in_the_personal_board() {
    let personal = Personal::new();
    let work = uncommitted();
    personal.enable(work.workdir(), &["boundary"]);
    private_names(&personal);
    names_boundary(&personal, &json!({}));
    let (ran, document) =
        personal.call_in(work.workdir(), "check", &pr(&json!({"pr.body": "Fine."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let path = output(&document)["crossing"].as_str().unwrap();
    assert_eq!(output(&document)["boundaries"], json!(["/boundaries/octo"]));
    assert!(personal.record(path).is_some());
    let file = work
        .workdir()
        .join(format!("{}.json", path.trim_start_matches('/')));
    assert!(!file.exists());
}

#[test]
fn enabling_writes_both_schemas_so_a_boundary_with_a_check_is_whole_and_one_without_is_invalid() {
    let personal = Personal::new();
    personal.write(
        "controls/names.json",
        r#"{"$type": "controls/control/v1", "requirement": "R.", "purpose": "P."}"#,
    );
    let fields =
        |checks: Value| json!({"to": "github:octo/*", "actions": ["github.*"], "checks": checks});
    personal
        .write("boundaries/ok.json", &boundary(&fields(names_check())))
        .write("boundaries/bad.json", &boundary(&fields(json!([]))));
    let report = json(&personal.run(&["--json", "check", "--all"]));
    let codes: BTreeMap<&str, &Value> = (report["data"].as_array().unwrap().iter())
        .map(|entry| (entry["logical"].as_str().unwrap(), &entry["code"]))
        .collect();
    assert_eq!(codes["/boundaries/ok"], &Value::Null);
    assert_eq!(
        codes["/.board/types/boundary/crossing/v1.schema"],
        &Value::Null
    );
    assert_eq!(codes["/boundaries/bad"], "board::record::invalid");
}

#[test]
fn a_check_naming_a_missing_control_dangles_and_one_naming_a_control_does_not() {
    let personal = Personal::new();
    personal.write(
        "controls/names.json",
        r#"{"$type": "controls/control/v1", "requirement": "R.", "purpose": "P."}"#,
    );
    for (name, control) in [("ok", "/controls/names"), ("gone", "/controls/gone")] {
        let checks = json!([{"control": control, "parts": ["*"]}]);
        let fields = json!({"to": "github:octo/*", "actions": ["*"], "checks": checks});
        personal.write(&format!("boundaries/{name}.json"), &boundary(&fields));
    }
    let report = json(&personal.run(&["--json", "check", "--all"]));
    let codes: BTreeMap<&str, &Value> = (report["data"].as_array().unwrap().iter())
        .map(|entry| (entry["logical"].as_str().unwrap(), &entry["code"]))
        .collect();
    assert_eq!(codes["/boundaries/ok"], &Value::Null);
    assert_eq!(codes["/boundaries/gone"], "board::ref::dangling");
}
