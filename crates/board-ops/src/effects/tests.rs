//! The effects check on puts, `decide` and `answer`, each refusal beside an
//! admitted twin.

use std::path::{Path, PathBuf};

use board_model::{
    Answer, Authority, Code, Digest, Effect, Layer, LogicalPath, Seen, canonical_json,
};
use board_scenario::seed::mount_of;
use board_scenario::signer::Signer;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{Asker, EffectError, Signing, answer, hold};
use crate::Board;
use crate::put::{Expect, PutError, PutReport, Removing, Unmatched};
use crate::tests::{TASK_SCHEMA, task};

const CONFIG: &str = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;

/// The test board, which does not commit.
fn board() -> Scenario {
    let scenario = crate::tests::board();
    scenario.write(".board/board.json", Item::File(CONFIG));
    scenario
}

/// A personal board, empty but for its configuration and the task type.
fn personal() -> Scenario {
    let scenario = Scenario::new();
    scenario
        .write(".board/board.json", Item::File(CONFIG))
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        );
    scenario
}

fn preference(path: &str, authority: &str) -> String {
    rule("put", path, authority)
}

fn rule(op: &str, path: &str, authority: &str) -> String {
    json!({"$type": "board/preference/v1", "op": op, "path": path, "authority": authority})
        .to_string()
}

/// `scenario` with a board rule at `.board/settings/<name>.json`.
fn setting<'s>(scenario: &'s Scenario, name: &str, path: &str, authority: &str) -> &'s Scenario {
    scenario.write(
        &format!(".board/settings/{name}.json"),
        Item::File(&preference(path, authority)),
    )
}

thread_local! {
    /// One signer for each test thread: a software key in place of Touch ID.
    static SIGNER: &'static Signer = Box::leak(Box::new(Signer::new()));
}

fn signer() -> &'static Signer {
    SIGNER.with(|signer| *signer)
}

/// How the test signer signs, and the allowed signers file that lists it.
fn signing() -> Signing {
    Signing {
        signer: Some(signer().program().into_os_string()),
        allowed: Some(signer().allowed()),
        cache: Some(signer().directory().join("cache")),
        environment: Vec::new(),
    }
}

fn asker(plugin: Option<&str>, personal: Option<&Scenario>) -> Asker {
    Asker {
        plugin: plugin.map(str::to_owned),
        personal: personal.map(|personal| personal.workdir().to_path_buf()),
        signing: signing(),
        ..Asker::default()
    }
}

/// `scenario` opened as `plugin` would open it, with `personal`.
fn opened(scenario: &Scenario, plugin: Option<&str>, personal: Option<&Scenario>) -> Board {
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.ask_as(asker(plugin, personal));
    board
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

fn put(board: &Board, at: &str, record: &str) -> Result<PutReport, EffectError> {
    effect(board.put(&path(at), Expect::Any, record.as_bytes()))
}

fn rm(board: &Board, at: &str) -> Result<PutReport, EffectError> {
    effect(board.remove_waiting(
        &path(at),
        Removing::File(Expect::Any),
        Unmatched::Refuse,
        board_tree::LOCK_WAIT,
    ))
}

/// The effects refusal of a put.
fn effect(put: Result<PutReport, PutError>) -> Result<PutReport, EffectError> {
    put.map_err(|error| match error {
        PutError::Effect(effect) => *effect,
        other => panic!("not an effects refusal: {other}"),
    })
}

fn code<T: std::fmt::Debug>(result: Result<T, EffectError>) -> Code {
    result.unwrap_err().code()
}

fn exists(scenario: &Scenario, physical: &str) -> bool {
    scenario.workdir().join(physical).exists()
}

fn read(scenario: &Scenario, physical: &str) -> Value {
    serde_json::from_slice(&std::fs::read(scenario.workdir().join(physical)).unwrap()).unwrap()
}

/// The id of the pending effect an ask refusal names.
fn asked(result: Result<PutReport, EffectError>) -> String {
    match result.unwrap_err() {
        EffectError::Asked { id, .. } => id,
        other => panic!("not asked: {other}"),
    }
}

fn link(to: &str) -> String {
    format!(r#"{{"$type": "board/mount/v1", "path": "{to}"}}"#)
}

#[test]
fn a_put_a_setting_asks_for_writes_a_pending_effect_and_is_refused_and_a_put_elsewhere_is_admitted()
{
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/a", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let refused = put(&board, "/tasks/a", &task(""));
    let fields = refused.as_ref().unwrap_err().fields();
    assert_eq!(refused.as_ref().unwrap_err().code(), Code::SettingsAsked);
    let id = fields.effect.unwrap();
    assert_eq!(id.len(), 64);
    assert!(!exists(&scenario, "tasks/a.json"));
    let effect = read(&mine, &format!("effects/pending/{id}.json"));
    assert_eq!(effect["plugin"], "tasks");
    assert_eq!(effect["path"], "/tasks/a");
    assert_eq!(effect["record"]["title"], "t");
    put(&board, "/tasks/b", &task("")).unwrap();
    assert!(exists(&scenario, "tasks/b.json"));
}

#[test]
fn an_invalid_record_at_a_path_that_asks_is_refused_as_invalid_with_no_pending_effect() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let invalid = board
        .put(
            &path("/tasks/a"),
            Expect::Any,
            br#"{"$type": "lr/task/v1"}"#,
        )
        .unwrap_err();
    assert_eq!(invalid.code(), Code::RecordInvalid);
    assert!(!exists(&mine, "effects"));
    assert_eq!(
        code(put(&board, "/tasks/a", &task(""))),
        Code::SettingsAsked
    );
    assert!(exists(&mine, "effects/pending"));
}

#[test]
fn an_approved_effect_lets_the_same_put_write_and_a_refused_one_is_refused_naming_it() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let approved = asked(put(&board, "/tasks/a", &task("")));
    answer(&owner, &approved, Answer::Approve, None, false).unwrap();
    put(&board, "/tasks/a", &task("")).unwrap();
    assert!(exists(&scenario, "tasks/a.json"));
    let refused = asked(put(&board, "/tasks/r", &task("")));
    answer(&owner, &refused, Answer::Refuse, None, false).unwrap();
    let error = put(&board, "/tasks/r", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SettingsRefused);
    assert_eq!(error.fields().effect.as_deref(), Some(refused.as_str()));
    let hint = error.refusal().hint.unwrap();
    assert!(
        hint.contains(&format!("`board answer {refused} approve`")),
        "{hint}"
    );
    // Another record at the approved path is another request.
    let changed = task(r#", "project": "/projects/p""#);
    assert_eq!(code(put(&board, "/tasks/a", &changed)), Code::SettingsAsked);
}

#[test]
fn an_approval_does_not_admit_the_same_put_over_another_blob() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    answer(&owner, &id, Answer::Approve, None, false).unwrap();
    let other = task(r#", "project": "/projects/p""#);
    scenario.write("tasks/a.json", Item::File(&other));
    assert_eq!(
        code(put(&board, "/tasks/a", &task(""))),
        Code::SettingsAsked
    );
    scenario.write("tasks/a.json", Item::Deleted);
    put(&board, "/tasks/a", &task("")).unwrap();
}

#[test]
fn a_refusing_rule_refuses_the_put_in_any_ascii_case_and_a_put_it_does_not_cover_is_admitted() {
    let scenario = board();
    setting(&scenario, "no", "/tasks/r*", "refuse");
    let board = opened(&scenario, Some("tasks"), None);
    assert_eq!(
        code(put(&board, "/tasks/R1", &task(""))),
        Code::SettingsRefused
    );
    put(&board, "/tasks/s1", &task("")).unwrap();
}

#[test]
fn a_put_that_may_only_propose_is_refused_without_or_propose_and_writes_a_pending_effect_with_it() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "propose");
    let mut board = opened(&scenario, Some("tasks"), Some(&mine));
    assert_eq!(
        code(put(&board, "/tasks/p", &task(""))),
        Code::SettingsProposeOnly
    );
    board.ask_as(Asker {
        or_propose: true,
        ..asker(Some("tasks"), Some(&mine))
    });
    let report = put(&board, "/tasks/p", &task("")).unwrap();
    let id = report.proposed.unwrap();
    assert!(report.writes.is_empty());
    assert!(!exists(&scenario, "tasks/p.json"));
    let effect = read(&mine, &format!("effects/pending/{id}.json"));
    assert_eq!(effect["authority"], "propose");
    // The owner's approval of a proposal lets the same put write.
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, false).unwrap();
    let report = put(&board, "/tasks/p", &task("")).unwrap();
    assert_eq!(report.proposed, None);
    assert!(exists(&scenario, "tasks/p.json"));
}

#[test]
fn a_plugins_put_into_settings_is_refused_whatever_its_type_and_a_task_elsewhere_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    let rule = preference("/*", "act");
    for record in [rule.as_str(), &task("")] {
        for at in ["/.board/settings/a", "/.board/Settings/b"] {
            let refused = put(&plugin, at, record);
            assert_eq!(code(refused), Code::SettingsFromPlugin, "{at}");
        }
    }
    put(&owner, "/.board/settings/a", &rule).unwrap();
    put(&plugin, "/rules/a", &task("")).unwrap();
    let file = |at: &str| effect(plugin.put_file(&path(at), Expect::Any, b"x"));
    assert_eq!(
        code(file("/.board/settings/a.md")),
        Code::SettingsFromPlugin
    );
    file("/notes/a.md").unwrap();
}

#[test]
fn a_plugins_put_into_preferences_or_effects_is_refused_in_the_personal_board_and_another_and_a_put_elsewhere_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    mine.write("preferences/x.json", Item::File(&preference("/*", "act")));
    let inside = opened(&mine, Some("tasks"), Some(&mine));
    for at in ["/preferences/x", "/effects/pending/x", "/effects/seen/x"] {
        assert_eq!(
            code(put(&inside, at, &task(""))),
            Code::SettingsFromPlugin,
            "{at}"
        );
    }
    put(&inside, "/notes/x", &task("")).unwrap();
    let elsewhere = opened(&scenario, Some("tasks"), Some(&mine));
    assert_eq!(
        code(put(&elsewhere, "/preferences/x", &task(""))),
        Code::SettingsFromPlugin
    );
    put(&elsewhere, "/notes/x", &task("")).unwrap();
}

#[test]
fn a_plugins_put_of_a_preference_effect_or_seen_record_is_refused_in_any_board_and_the_owners_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    let effect = Effect::new(
        "/b".to_owned(),
        "tasks".to_owned(),
        "put".to_owned(),
        "/x".to_owned(),
    );
    let seen = json!({"$type": "board/seen/v1", "effect": "0".repeat(64), "answer": "approve"});
    let records = [
        preference("/*", "act"),
        serde_json::to_string(&effect).unwrap(),
        seen.to_string(),
    ];
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    let owner = opened(&scenario, None, Some(&mine));
    for (at, record) in ["/notes/a", "/notes/b", "/notes/c"].iter().zip(&records) {
        let refused = put(&plugin, at, record).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        assert!(refused.to_string().contains("record"), "{refused}");
        put(&owner, at, record).unwrap();
    }
    put(&plugin, "/notes/d", &task("")).unwrap();
}

#[test]
fn a_plugins_single_put_under_types_is_refused_and_a_batch_schema_line_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("tasks"), None);
    let open = json!({"type": "object"});
    // A task is also a schema, one that admits any record.
    for at in ["lr/note", "lr/task"] {
        let at = format!("/.board/types/{at}/v1.schema");
        let refused = put(&plugin, &at, &task("")).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
    }
    let line = |name: &str| {
        let line = json!({"path": format!("/.board/types/lr/{name}/v1.schema"), "record": open});
        format!("{line}\n")
    };
    let changed = plugin.put_batch(&mut line("task").as_bytes()).unwrap_err();
    assert_eq!(lines(&changed), [(1, Code::SchemaDiffers)]);
    plugin.put_batch(&mut line("note").as_bytes()).unwrap();
    assert!(exists(&scenario, ".board/types/lr/note/v1.schema.json"));
}

#[test]
fn the_personal_board_named_through_a_symlink_is_still_the_personal_board() {
    let (mine, elsewhere) = (personal(), Scenario::new());
    let named = elsewhere.workdir().join("me");
    std::os::unix::fs::symlink(mine.workdir(), &named).unwrap();
    let mut inside = Board::open(mine.workdir(), None).unwrap();
    inside.ask_as(Asker {
        personal: Some(named),
        ..asker(Some("tasks"), None)
    });
    assert_eq!(
        code(put(&inside, "/preferences/x", &task(""))),
        Code::SettingsFromPlugin
    );
    put(&inside, "/rules/x", &task("")).unwrap();
}

#[test]
fn a_plugins_put_through_a_link_into_a_protected_file_is_refused_and_through_another_link_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    let strict = preference("/z", "refuse");
    scenario
        .write(".board/settings/strict.json", Item::File(&strict))
        .write(
            "notes/strict.json",
            Item::File(&link("/.board/settings/strict")),
        )
        .write("notes/project.json", Item::File(&link("/projects/p")));
    let plugin = opened(&scenario, Some("tasks"), None);
    let project = r#"{"$type": "lr/project/v1"}"#;
    let refused = put(&plugin, "/notes/strict", project);
    assert_eq!(code(refused), Code::SettingsFromPlugin);
    put(&plugin, "/notes/project", project).unwrap();
    mine.write("preferences/x.json", Item::File(&preference("/*", "act")))
        .write("notes/x.json", Item::File(&link("/preferences/x")));
    let inside = opened(&mine, Some("tasks"), Some(&mine));
    let rule = preference("/*", "refuse");
    assert_eq!(
        code(put(&inside, "/notes/x", &rule)),
        Code::SettingsFromPlugin
    );
}

#[test]
fn a_plugins_put_of_a_link_is_refused_and_the_owners_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    let to = link("/.board/settings/strict");
    assert_eq!(
        code(put(&plugin, "/notes/l", &to)),
        Code::SettingsFromPlugin
    );
    put(&owner, "/notes/l", &to).unwrap();
}

#[test]
fn a_rule_saved_for_one_board_does_not_cover_a_put_in_another() {
    let (scenario, other, mine) = (board(), board(), personal());
    let refused = opened(&scenario, Some("tasks"), Some(&mine));
    let root = refused.repository().path().display().to_string();
    let mut rule: Value = serde_json::from_str(&preference("/tasks/*", "refuse")).unwrap();
    rule["board"] = json!(root);
    mine.write("preferences/one.json", Item::File(&rule.to_string()));
    assert_eq!(
        code(put(&refused, "/tasks/a", &task(""))),
        Code::SettingsRefused
    );
    let admitted = opened(&other, Some("tasks"), Some(&mine));
    put(&admitted, "/tasks/a", &task("")).unwrap();
}

/// A board in its own repository that commits its puts, with a task schema,
/// a rule refusing `/notes/z` and an identity, and the test board mounting
/// it at `/m`.
fn mounted() -> (Scenario, Scenario) {
    let library = Scenario::new();
    let strict = preference("/notes/z", "refuse");
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/settings/strict.json", strict.as_str()),
    ];
    for (at, text) in files {
        library.write(at, Item::File(text));
    }
    let commit = library.commit(&files);
    library.track(commit).write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    let scenario = board();
    scenario.write("m.json", Item::File(&mount_of(&library.path(), commit)));
    (scenario, library)
}

#[test]
fn a_plugins_put_through_a_mount_meets_the_mounted_boards_settings_at_its_inner_path() {
    let (scenario, library) = mounted();
    // The root's own rule names a root path, where a put through the mount
    // does not land.
    setting(&scenario, "root", "/m/*", "refuse");
    let plugin = opened(&scenario, Some("tasks"), None);
    let rule = preference("/*", "act");
    let settings = put(&plugin, "/m/.board/settings/strict", &rule);
    assert_eq!(code(settings), Code::SettingsFromPlugin);
    assert_eq!(
        code(put(&plugin, "/m/notes/z", &task(""))),
        Code::SettingsRefused
    );
    put(&plugin, "/m/notes/y", &task("")).unwrap();
    assert!(library.head().unwrap().files.contains_key("notes/y.json"));
}

#[test]
fn a_plugins_file_put_through_a_mount_that_the_mounted_board_refuses_uploads_nothing_and_its_twin_uploads()
 {
    let (scenario, library) = mounted();
    let config = json!({
        "$type": "board/config/v1",
        "commit_on_put": false,
        "store": {"backend": "directory", "location": "store"},
    });
    scenario.write(".board/board.json", Item::File(&config.to_string()));
    let plugin = opened(&scenario, Some("tasks"), None);
    let bytes = b"\xff\xfe";
    let refused = effect(plugin.put_file(&path("/m/notes/z"), Expect::Any, bytes));
    assert_eq!(code(refused), Code::SettingsRefused);
    assert!(!exists(&scenario, "store"));
    plugin
        .put_file(&path("/m/notes/y"), Expect::Any, bytes)
        .unwrap();
    assert!(exists(
        &scenario,
        &format!("store/{}", Digest::of(bytes).hex())
    ));
    assert!(library.head().unwrap().files.contains_key("notes/y.json"));
}

#[test]
fn a_preference_whose_pattern_has_an_empty_or_dot_segment_is_refused_and_its_twin_is_admitted() {
    let mine = personal();
    let owner = opened(&mine, None, None);
    for pattern in ["/notes//x", "/notes/./x", "/notes/../x", "/notes/"] {
        let refused = owner
            .put(
                &path("/preferences/x"),
                Expect::Any,
                preference(pattern, "act").as_bytes(),
            )
            .unwrap_err();
        assert_eq!(refused.code(), Code::RecordInvalid, "{pattern}");
        let message = refused.to_string();
        assert!(
            message.contains("not a valid board/preference/v1")
                && message.contains(&format!("{pattern:?}")),
            "{message}"
        );
        let hint = refused.refusal().hint.unwrap_or_default();
        assert!(hint.contains("Board's own type"), "{hint}");
    }
    put(&owner, "/preferences/x", &preference("/notes/x", "act")).unwrap();
}

#[test]
fn an_ask_with_no_personal_board_is_refused_as_no_personal_and_an_act_is_admitted() {
    let scenario = board();
    setting(&scenario, "tasks", "/tasks/a", "ask");
    let board = opened(&scenario, Some("tasks"), None);
    let refused = put(&board, "/tasks/a", &task("")).unwrap_err();
    assert_eq!(refused.code(), Code::SettingsNoPersonal);
    assert_eq!(
        refused.to_string(),
        "put /tasks/a needs a pending effect in the personal board, but the known boards file names no personal board"
    );
    put(&board, "/tasks/b", &task("")).unwrap();
}

fn line(at: &str) -> String {
    json!({"path": at, "record": serde_json::from_str::<Value>(&task("")).unwrap()}).to_string()
}

/// Each line a refused batch names, with its code.
fn lines(refused: &PutError) -> Vec<(usize, Code)> {
    (refused.fields().refusals.iter())
        .map(|lined| (lined.line, lined.code))
        .collect()
}

#[test]
fn a_batch_with_a_line_that_asks_is_refused_naming_the_line_and_its_twin_is_admitted() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/a", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let batch = |at: &str| format!("{}\n{}\n", line("/tasks/c"), line(at));
    let refused = board
        .put_batch(&mut batch("/tasks/a").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutInvalid);
    assert_eq!(lines(&refused), [(2, Code::SettingsAsked)]);
    assert!(!exists(&scenario, "tasks/c.json"));
    assert!(exists(&mine, "effects/pending"));
    board.put_batch(&mut batch("/tasks/b").as_bytes()).unwrap();
    assert!(exists(&scenario, "tasks/c.json"));
}

#[test]
fn a_batch_with_a_line_that_may_only_propose_is_refused_and_its_twin_is_admitted() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/p", "propose");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let refused = board
        .put_batch(&mut line("/tasks/p").as_bytes())
        .unwrap_err();
    assert_eq!(lines(&refused), [(1, Code::SettingsProposeOnly)]);
    board.put_batch(&mut line("/tasks/q").as_bytes()).unwrap();
}

#[test]
fn a_file_put_that_asks_writes_a_pending_effect_with_its_text_and_its_twin_is_admitted() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "notes", "/notes/a.md", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let file = |at: &str| effect(board.put_file(&path(at), Expect::Any, b"# a\n"));
    let id = asked(file("/notes/a.md"));
    let pending = read(&mine, &format!("effects/pending/{id}.json"));
    assert_eq!(pending["text"], "# a\n");
    file("/notes/b.md").unwrap();
}

#[test]
fn deciding_a_put_reports_its_authority_and_writes_nothing_and_another_op_asks_under_a_plugin() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "wide", "/x*", "ask");
    setting(&scenario, "narrow", "/x", "refuse");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let report = board.authority("put", &path("/x")).unwrap();
    assert_eq!(report.decision.authority, Authority::Refuse);
    assert_eq!(report.decision.matching.len(), 2);
    let deciding = &report.decision.matching[report.decision.deciding.unwrap()];
    assert_eq!(
        (deciding.layer, deciding.at.as_str()),
        (Layer::Board, "/.board/settings/narrow")
    );
    let put = board.authority("put", &path("/xy")).unwrap();
    assert_eq!((put.decision.authority, put.effect), (Authority::Ask, None));
    assert!(!exists(&mine, "effects"));
    let owner = opened(&scenario, None, Some(&mine));
    let unasked = owner.authority("review.comment", &path("/y")).unwrap();
    assert_eq!(
        (unasked.decision.authority, unasked.effect),
        (Authority::Ask, None)
    );
    assert!(!exists(&mine, "effects"));
    let asked = board.authority("review.comment", &path("/y")).unwrap();
    assert_eq!(asked.decision.authority, Authority::Ask);
    let id = asked.effect.unwrap();
    assert!(exists(&mine, &format!("effects/pending/{id}.json")));
}

#[test]
fn an_answer_with_save_rule_writes_the_seen_record_and_a_preference_for_its_board_and_decide_then_acts()
 {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let asked = board.authority("review.comment", &path("/y")).unwrap();
    let id = asked.effect.unwrap();
    let owner = asker(None, Some(&mine));
    let report = answer(&owner, &id, Answer::Approve, Some("fine".to_owned()), true).unwrap();
    assert_eq!(report.rule.as_deref(), Some(&*format!("/preferences/{id}")));
    let root = board.repository().path().display().to_string();
    assert_eq!(
        read(&mine, &format!("preferences/{id}.json")),
        json!({"$type": "board/preference/v1", "op": "review.comment", "path": "/y", "authority": "act", "plugin": "tasks", "board": root})
    );
    let seen = read(&mine, &format!("effects/seen/{id}.json"));
    assert_eq!(seen["reason"], "fine");
    // The saved rule now decides, so the answer is not read.
    let decided = board.authority("review.comment", &path("/y")).unwrap();
    assert_eq!(
        (decided.decision.authority, decided.seen),
        (Authority::Act, None)
    );
    let deciding = &decided.decision.matching[decided.decision.deciding.unwrap()];
    assert_eq!(deciding.at, format!("/preferences/{id}"));
}

#[test]
fn an_answer_removes_its_pending_effect_in_the_commit_that_writes_the_seen_record_and_an_unanswered_one_stays()
 {
    let scenario = board();
    let mine = Scenario::new();
    mine.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1"}"#),
    )
    .write(
        ".git/config",
        Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
    );
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let answered = asked(put(&board, "/tasks/a", &task("")));
    let unanswered = asked(put(&board, "/tasks/b", &task("")));
    let pending = |id: &str| format!("effects/pending/{id}.json");
    let before = mine.head().unwrap();
    let owner = asker(None, Some(&mine));
    let report = answer(&owner, &answered, Answer::Approve, None, false).unwrap();
    let head = mine.head().unwrap();
    assert_eq!(report.put.commit, Some(head.id));
    assert_eq!(head.parents, [before.id]);
    assert_eq!(head.message, format!("board answer {answered}"));
    assert!(
        head.files
            .contains_key(&format!("effects/seen/{answered}.json"))
    );
    assert!(!head.files.contains_key(&pending(&answered)));
    assert!(!exists(&mine, &pending(&answered)));
    assert!(head.files.contains_key(&pending(&unanswered)));
    assert!(exists(&mine, &pending(&unanswered)));
    // The approved put finds the seen record by its id and writes.
    put(&board, "/tasks/a", &task("")).unwrap();
}

#[test]
fn a_second_answer_to_the_same_effect_is_refused_as_not_found_and_the_first_is_admitted() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    let owner = asker(None, Some(&mine));
    answer(&owner, &id, Answer::Refuse, None, false).unwrap();
    let again = answer(&owner, &id, Answer::Approve, None, false);
    assert_eq!(code(again), Code::SettingsNotFound);
    assert_eq!(
        read(&mine, &format!("effects/seen/{id}.json"))["answer"],
        "refuse"
    );
}

/// A put reads no answer, the owner refuses, and then the put holds its
/// effect: the hold writes nothing, and a pending effect left over from
/// before cannot turn the refusal into an approval.
#[test]
fn a_hold_after_the_owner_answers_writes_no_pending_effect_and_a_hold_before_writes_one() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    let file = format!("effects/pending/{id}.json");
    let text = std::fs::read_to_string(mine.workdir().join(&file)).unwrap();
    let effect = Effect::from_value(&serde_json::from_str(&text).unwrap()).unwrap();
    let personal = Board::open(mine.workdir(), None).unwrap();
    answer(&owner, &id, Answer::Refuse, None, false).unwrap();
    hold(&personal, &effect).unwrap();
    assert!(!exists(&mine, &file));
    mine.write(&file, Item::File(&text));
    let again = answer(&owner, &id, Answer::Approve, None, false);
    assert_eq!(code(again), Code::PutStale);
    assert_eq!(
        read(&mine, &format!("effects/seen/{id}.json"))["answer"],
        "refuse"
    );
    let unanswered = asked(put(&board, "/tasks/b", &task("")));
    let file = format!("effects/pending/{unanswered}.json");
    let text = std::fs::read_to_string(mine.workdir().join(&file)).unwrap();
    let effect = Effect::from_value(&serde_json::from_str(&text).unwrap()).unwrap();
    mine.write(&file, Item::Deleted);
    hold(&personal, &effect).unwrap();
    assert!(exists(&mine, &file));
}

#[test]
fn answering_from_a_plugin_without_a_personal_board_or_for_an_unknown_or_malformed_id_is_refused_and_its_twin_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    let owner = asker(None, Some(&mine));
    let approve = |asker: &Asker, id: &str| answer(asker, id, Answer::Approve, None, false);
    let plugin = asker(Some("tasks"), Some(&mine));
    assert_eq!(code(approve(&plugin, &id)), Code::SettingsFromPlugin);
    assert_eq!(
        code(approve(&asker(None, None), &id)),
        Code::SettingsNoPersonal
    );
    let unknown = "0".repeat(64);
    assert_eq!(code(approve(&owner, &unknown)), Code::SettingsNotFound);
    assert_eq!(code(approve(&owner, &id[..16])), Code::SettingsNotFound);
    let upper = id.to_uppercase();
    assert_eq!(code(approve(&owner, &upper)), Code::SettingsNotFound);
    assert!(!exists(&mine, "effects/seen"));
    assert_eq!(approve(&owner, &id).unwrap().rule, None);
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
}

/// Writes `effect` in `mine` as the pending effect `id`.
fn pending(mine: &Scenario, id: &str, effect: &Effect) {
    let value = serde_json::to_string(effect).unwrap();
    mine.write(&format!("effects/pending/{id}.json"), Item::File(&value));
}

fn request(op: &str, at: &str) -> Effect {
    Effect::new(
        "/b".to_owned(),
        "tasks".to_owned(),
        op.to_owned(),
        at.to_owned(),
    )
}

#[test]
fn an_answer_to_a_pending_effect_that_is_not_the_one_its_id_names_is_refused_and_the_true_one_is_admitted()
 {
    let mine = personal();
    let owner = asker(None, Some(&mine));
    let approve = |id: &str| answer(&owner, id, Answer::Approve, None, false);
    let mut effect = request("put", "/x");
    effect.record = Some(json!({"$type": "lr/task/v1", "title": "t"}));
    effect.digest = Some(Digest::of(b"{}"));
    let id = effect.id();
    pending(&mine, &id, &effect);
    let refused = approve(&id).unwrap_err();
    assert_eq!(refused.code(), Code::SettingsNotFound);
    assert!(refused.to_string().contains("its digest"), "{refused}");
    let mut other = effect.clone();
    other.path = "/y".to_owned();
    pending(&mine, &id, &other);
    let refused = approve(&id).unwrap_err();
    assert!(refused.to_string().contains("has the id"), "{refused}");
    let shown = canonical_json(effect.record.as_ref().unwrap());
    effect.digest = Some(Digest::of(shown.as_bytes()));
    let id = effect.id();
    pending(&mine, &id, &effect);
    approve(&id).unwrap();
}

#[test]
fn saving_a_rule_for_a_path_no_pattern_names_exactly_is_refused_saying_why_and_its_twin_is_admitted()
 {
    let mine = personal();
    let owner = asker(None, Some(&mine));
    let asked = |at: &str| {
        let effect = request("c.d", at);
        let id = effect.id();
        pending(&mine, &id, &effect);
        id
    };
    let save = |id: &str| answer(&owner, id, Answer::Refuse, None, true);
    for (at, why) in [("/x*", "* or ?"), ("/x[1]", "'['")] {
        let refused = save(&asked(at)).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsBadPattern, "{at}");
        assert!(refused.to_string().contains(why), "{refused}");
    }
    assert!(!exists(&mine, "preferences"));
    save(&asked("/x1")).unwrap();
    assert!(exists(&mine, "preferences"));
}

#[test]
fn a_plugins_put_whose_path_is_not_ascii_is_refused_and_its_ascii_twin_is_admitted() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "secret", "/secret/*", "refuse");
    scenario.write("secret/a.json", Item::File(&task("")));
    mine.write("preferences/x.json", Item::File(&preference("/*", "act")));
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    let inside = opened(&mine, Some("tasks"), Some(&mine));
    // `ſ` folds to `s` and the Kelvin sign to `k` on a case-insensitive
    // file system, so each would land on the protected or refused file.
    for (board, at) in [
        (&plugin, "/.board/ſettings/x"),
        (&plugin, "/ſecret/x"),
        (&inside, "/preferenceſ/y"),
        (&inside, "/effectſ/seen/y"),
        (&plugin, "/\u{212A}ept/x"),
    ] {
        let refused = put(board, at, &task("")).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        assert!(refused.to_string().contains("not ASCII"), "{refused}");
    }
    put(&plugin, "/kept/x", &task("")).unwrap();
    put(&inside, "/notes/y", &task("")).unwrap();
}

#[test]
fn a_plugins_put_in_another_ascii_case_of_an_existing_directory_is_refused_as_a_case_collision_and_its_twin_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "secret", "/secret/*", "refuse");
    scenario.write("secret/a.json", Item::File(&task("")));
    mine.write("preferences/x.json", Item::File(&preference("/*", "act")));
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    let inside = opened(&mine, Some("tasks"), Some(&mine));
    let rule = preference("/*", "act");
    // A new directory differing only in case is refused before the check.
    let collides = |board: &Board, at: &str| board.put(&path(at), Expect::Any, rule.as_bytes());
    let refused = collides(&plugin, "/SeCrEt/b").unwrap_err();
    assert_eq!(refused.code(), Code::PathCaseCollision);
    let protected = collides(&inside, "/Preferences/y").unwrap_err();
    assert_eq!(protected.code(), Code::PathCaseCollision);
    put(&plugin, "/open/b", &task("")).unwrap();
    put(&inside, "/notes/y", &task("")).unwrap();
}

#[test]
fn the_landing_of_a_path_is_spelled_as_the_file_system_spells_its_existing_ancestor() {
    let scenario = board();
    scenario.write("secret/a.json", Item::File(&task("")));
    let board = opened(&scenario, None, None);
    let folds = scenario.workdir().join("SECRET").exists();
    let landing = board.repository().landing("SECRET/new/b.json");
    let expected = if folds {
        "secret/new/b.json"
    } else {
        "SECRET/new/b.json"
    };
    assert_eq!(landing, expected);
    assert_eq!(board.repository().landing("open/b.json"), "open/b.json");
}

#[test]
fn a_rule_file_that_is_not_a_preference_refuses_a_plugins_put_naming_it_and_the_owners_is_admitted()
{
    let (scenario, mine) = (board(), personal());
    for broken in [
        r#"{"$type": "board/preference/v1", "op": "put"}"#,
        r#"{"$type": "lr/task/v1", "title": "t"}"#,
        "{",
    ] {
        scenario.write(".board/settings/broken.json", Item::File(broken));
        let plugin = opened(&scenario, Some("tasks"), Some(&mine));
        let refused = put(&plugin, "/tasks/a", &task("")).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsBadRule, "{broken}");
        assert!(refused.to_string().contains("broken.json"), "{refused}");
        let decided = plugin.authority("review.comment", &path("/y"));
        assert_eq!(code(decided), Code::SettingsBadRule);
        put(&opened(&scenario, None, Some(&mine)), "/tasks/a", &task("")).unwrap();
    }
    setting(&scenario, "broken", "/z", "refuse");
    put(
        &opened(&scenario, Some("tasks"), Some(&mine)),
        "/tasks/a",
        &task(""),
    )
    .unwrap();
}

#[test]
fn a_file_put_that_asks_with_bytes_that_are_not_utf8_is_refused_as_cannot_hold_and_text_asks() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "notes", "/notes/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let file = |bytes: &[u8]| effect(board.put_file(&path("/notes/a.md"), Expect::Any, bytes));
    assert_eq!(code(file(b"\xff\xfe")), Code::SettingsCannotHold);
    assert!(!exists(&mine, "effects"));
    asked(file(b"# a\n"));
}

#[test]
fn approving_a_put_with_save_rule_is_refused_as_cannot_widen_and_refusing_one_saves_the_rule() {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    let owner = asker(None, Some(&mine));
    let widen = answer(&owner, &id, Answer::Approve, None, true);
    assert_eq!(code(widen), Code::SettingsCannotWiden);
    assert!(!exists(&mine, "effects/seen"));
    let saved = answer(&owner, &id, Answer::Refuse, None, true).unwrap();
    assert_eq!(saved.rule, Some(format!("/preferences/{id}")));
}

#[test]
fn deciding_through_a_link_or_a_mount_decides_where_a_put_lands_and_its_twin_does_not_match() {
    let (scenario, library) = mounted();
    setting(&scenario, "tasks", "/tasks/*", "refuse");
    scenario
        .write("l.json", Item::File(&link("/tasks")))
        .write("tasks/b.json", Item::File(&task("")));
    let _ = library;
    let plugin = opened(&scenario, Some("tasks"), None);
    let decided = |at: &str| {
        plugin
            .authority("put", &path(at))
            .unwrap()
            .decision
            .authority
    };
    assert_eq!(decided("/l/a"), Authority::Refuse);
    assert_eq!(decided("/m/notes/z"), Authority::Refuse);
    assert_eq!(decided("/m/notes/y"), Authority::Act);
    assert_eq!(decided("/k/a"), Authority::Act);
}

#[test]
fn a_rule_naming_its_board_through_a_symlink_covers_puts_there_and_one_naming_another_does_not() {
    let (scenario, mine, elsewhere) = (board(), personal(), Scenario::new());
    let named = elsewhere.workdir().join("b");
    std::os::unix::fs::symlink(scenario.workdir(), &named).unwrap();
    let mut rule: Value = serde_json::from_str(&preference("/tasks/*", "refuse")).unwrap();
    rule["board"] = json!(named.display().to_string());
    mine.write("preferences/one.json", Item::File(&rule.to_string()));
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    assert_eq!(
        code(put(&plugin, "/tasks/a", &task(""))),
        Code::SettingsRefused
    );
    rule["board"] = json!(elsewhere.workdir().display().to_string());
    mine.write("preferences/one.json", Item::File(&rule.to_string()));
    put(&plugin, "/tasks/a", &task("")).unwrap();
}

#[test]
fn a_batch_schema_line_under_a_plugin_meets_the_rules_at_its_schema_file_and_its_twin_is_admitted()
{
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    let schema = |name: &str| {
        let schema = json!({"type": "object"});
        format!(
            "{}\n",
            json!({"path": format!("/.board/types/lr/{name}/v1.schema"), "record": schema})
        )
    };
    setting(&scenario, "types", "/.board/types/lr/kept/*", "refuse");
    let refused = plugin
        .put_batch(&mut schema("kept").as_bytes())
        .unwrap_err();
    assert_eq!(lines(&refused), [(1, Code::SettingsRefused)]);
    plugin.put_batch(&mut schema("new").as_bytes()).unwrap();
    assert!(exists(&scenario, ".board/types/lr/new/v1.schema.json"));
    let config = put(&plugin, "/.board/board", r#"{"$type": "board/config/v1"}"#);
    assert_eq!(code(config), Code::SettingsFromPlugin);
}

/// The pending effect `id` in `mine`.
fn pending_effect(mine: &Scenario, id: &str) -> Effect {
    Effect::from_value(&read(mine, &format!("effects/pending/{id}.json"))).unwrap()
}

/// An approval of `id` written straight into `mine`, as an agent with the
/// owner's shell could write it, carrying `signature`.
fn approve_by_hand(mine: &Scenario, id: &str, signature: Option<String>) {
    let mut seen = Seen::new(format!("/effects/pending/{id}"), Answer::Approve, None);
    seen.signature = signature;
    let text = json!(seen).to_string();
    mine.write(&format!("effects/seen/{id}.json"), Item::File(&text));
}

/// The effect a refusal names, with its code.
fn named<T: std::fmt::Debug>(result: Result<T, EffectError>) -> (Code, Option<String>) {
    let error = result.unwrap_err();
    (error.code(), error.fields().effect)
}

#[test]
fn an_approval_without_the_owners_signature_or_by_another_key_is_refused_as_unsigned_and_the_owners_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let id = asked(put(&board, "/tasks/a", &task("")));
    let approval = pending_effect(&mine, &id).approval(false);
    let unsigned = (Code::SettingsUnsigned, Some(id.clone()));
    approve_by_hand(&mine, &id, None);
    assert_eq!(named(put(&board, "/tasks/a", &task(""))), unsigned);
    approve_by_hand(&mine, &id, Some(Signer::new().sign(&approval)));
    assert_eq!(named(put(&board, "/tasks/a", &task(""))), unsigned);
    let mut moved = pending_effect(&mine, &id);
    moved.path = "/tasks/b".to_owned();
    approve_by_hand(&mine, &id, Some(signer().sign(&moved.approval(false))));
    assert_eq!(named(put(&board, "/tasks/a", &task(""))), unsigned);
    approve_by_hand(&mine, &id, Some(signer().sign(&approval)));
    put(&board, "/tasks/a", &task("")).unwrap();
    assert!(exists(&scenario, "tasks/a.json"));
}

#[test]
fn deciding_an_op_whose_owner_approval_is_unsigned_is_refused_as_unsigned_and_a_signed_one_acts() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let id = board
        .authority("review.comment", &path("/y"))
        .unwrap()
        .effect
        .unwrap();
    let effect = pending_effect(&mine, &id);
    let approval = effect.approval(false);
    approve_by_hand(&mine, &id, None);
    assert_eq!(
        named(board.authority("review.comment", &path("/y"))),
        (Code::SettingsUnsigned, Some(id.clone()))
    );
    // A seen record that claims a saved rule the signed statement did not
    // show is refused too.
    let mut claims = Seen::new(format!("/effects/pending/{id}"), Answer::Approve, None);
    (claims.signature, claims.rule) = (
        Some(signer().sign(&approval)),
        Some(format!("/preferences/{id}")),
    );
    let claims = json!(claims).to_string();
    mine.write(&format!("effects/seen/{id}.json"), Item::File(&claims));
    assert_eq!(
        named(board.authority("review.comment", &path("/y"))),
        (Code::SettingsUnsigned, Some(id.clone()))
    );
    let mut saves = Seen::new(format!("/effects/pending/{id}"), Answer::Approve, None);
    (saves.signature, saves.rule) = (
        Some(signer().sign(&effect.approval(true))),
        Some(format!("/preferences/{id}")),
    );
    let saves = json!(saves).to_string();
    mine.write(&format!("effects/seen/{id}.json"), Item::File(&saves));
    board.authority("review.comment", &path("/y")).unwrap();
    approve_by_hand(&mine, &id, Some(signer().sign(&approval)));
    let decided = board.authority("review.comment", &path("/y")).unwrap();
    assert_eq!(
        (decided.decision.authority, decided.seen),
        (Authority::Act, Some(Answer::Approve))
    );
}

#[test]
fn approving_with_no_signer_is_refused_as_unsigned_and_writes_nothing_and_refusing_needs_no_signature()
 {
    let (scenario, mine) = (board(), personal());
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let unsigned_owner = Asker {
        signing: Signing::default(),
        ..asker(None, Some(&mine))
    };
    let id = asked(put(&board, "/tasks/a", &task("")));
    assert_eq!(
        named(answer(&unsigned_owner, &id, Answer::Approve, None, false)),
        (Code::SettingsUnsigned, Some(id.clone()))
    );
    assert!(exists(&mine, &format!("effects/pending/{id}.json")));
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, false).unwrap();
    let seen = read(&mine, &format!("effects/seen/{id}.json"));
    let signature = seen["signature"].as_str().unwrap();
    assert!(signature.starts_with("-----BEGIN SSH SIGNATURE-----"));
    let refused = asked(put(&board, "/tasks/r", &task("")));
    answer(&unsigned_owner, &refused, Answer::Refuse, None, false).unwrap();
    let seen = read(&mine, &format!("effects/seen/{refused}.json"));
    assert_eq!(
        (seen["answer"].as_str(), seen.get("signature")),
        (Some("refuse"), None)
    );
}

#[test]
fn approving_when_the_owner_declines_to_sign_is_refused_as_unsigned_with_their_feedback_and_signing_is_admitted()
 {
    let (scenario, mine, tools) = (board(), personal(), Scenario::new());
    tools.write(
        "decline",
        Item::Executable("#!/bin/sh\necho 'wrong path'\nexit 3\n"),
    );
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let mut declining = asker(None, Some(&mine));
    declining.signing.signer = Some(tools.workdir().join("decline").into_os_string());
    let id = asked(put(&board, "/tasks/a", &task("")));
    let error = answer(&declining, &id, Answer::Approve, None, false).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(error.to_string().contains("wrong path"), "{error}");
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, false).unwrap();
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
}

/// `scenario` opened by the owner, whose puts to a rule or an effect are
/// signed by `signer`, or by the test signer.
fn owned(scenario: &Scenario, mine: &Scenario, signer: Option<&Path>) -> Board {
    let mut owner = asker(None, Some(mine));
    owner.sign_owner_writes = true;
    if let Some(signer) = signer {
        owner.signing.signer = Some(signer.as_os_str().to_owned());
    }
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.ask_as(owner);
    board
}

/// A signer that declines, and one that keeps the statement it is shown
/// at `shown` before the test signer signs it.
fn owner_signers() -> (Scenario, PathBuf, PathBuf) {
    let tools = Scenario::new();
    let shown = tools.workdir().join("shown");
    tools
        .write(
            "decline",
            Item::Executable("#!/bin/sh\necho 'not this rule'\nexit 3\n"),
        )
        .write(
            "keep",
            Item::Executable(&format!(
                "#!/bin/sh\ncp \"$2\" '{}'\nexec '{}' \"$@\"\n",
                shown.display(),
                signer().program().display()
            )),
        );
    let (decline, keep) = (
        tools.workdir().join("decline"),
        tools.workdir().join("keep"),
    );
    (tools, decline, keep)
}

#[test]
fn an_owners_put_to_a_rule_or_an_effect_is_refused_unless_they_sign_it_and_a_put_elsewhere_needs_no_signature()
 {
    let (scenario, mine) = (board(), personal());
    let (tools, decline, keep) = owner_signers();
    let declining = owned(&mine, &mine, Some(&decline));
    let rule = preference("/tasks/*", "act");
    for (at, file) in [
        ("/preferences/r", "preferences/r.json"),
        ("/Preferences/r", "Preferences/r.json"),
        ("/effects/seen/s", "effects/seen/s.json"),
    ] {
        let error = put(&declining, at, &rule).unwrap_err();
        assert_eq!(error.code(), Code::SettingsUnsigned, "{at}");
        assert!(matches!(error, EffectError::OwnerUnsigned { .. }), "{at}");
        assert!(error.to_string().contains("not this rule"), "{error}");
        assert!(!exists(&mine, file), "{at}");
    }
    put(&declining, "/tasks/a", &task("")).unwrap();
    assert!(exists(&mine, "tasks/a.json"));
    let other = owned(&scenario, &mine, Some(&decline));
    let error = put(&other, "/.board/settings/r", &rule).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(!exists(&scenario, ".board/settings/r.json"));
    let signing = owned(&scenario, &mine, Some(&keep));
    put(&signing, "/.board/settings/r", &rule).unwrap();
    assert!(exists(&scenario, ".board/settings/r.json"));
    let shown = std::fs::read_to_string(tools.workdir().join("shown")).unwrap();
    let root = scenario.workdir().canonicalize().unwrap();
    assert!(
        shown.starts_with(&format!(
            "board/write/v1\nop: put\npath: /.board/settings/r\nboard: {}\ncontent: ",
            root.display()
        )),
        "{shown}"
    );
    assert!(shown.contains("\"authority\": \"act\""), "{shown}");
}

#[test]
fn an_owners_put_to_a_rule_signed_as_an_approval_is_refused() {
    let mine = personal();
    let tools = Scenario::new();
    let key = signer().directory().join("key");
    tools.write(
        "as-approval",
        Item::Executable(&format!(
            "#!/bin/sh\nexec /usr/bin/ssh-keygen -Y sign -q -f '{}' -n board/approve/v1 < \"$2\"\n",
            key.display()
        )),
    );
    let rule = preference("/tasks/*", "act");
    let as_approval = owned(&mine, &mine, Some(&tools.workdir().join("as-approval")));
    let error = put(&as_approval, "/preferences/r", &rule).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(!exists(&mine, "preferences/r.json"));
    put(&owned(&mine, &mine, None), "/preferences/r", &rule).unwrap();
    assert!(exists(&mine, "preferences/r.json"));
}

#[test]
fn an_owners_file_put_to_a_rule_needs_their_signature_and_a_signed_one_writes() {
    let mine = personal();
    let (_tools, decline, _keep) = owner_signers();
    let bytes = b"why the rules are as they are\n";
    let declining = owned(&mine, &mine, Some(&decline));
    let refused = declining.put_file(&path("/preferences/why.md"), Expect::Any, bytes);
    assert_eq!(effect(refused).unwrap_err().code(), Code::SettingsUnsigned);
    assert!(!exists(&mine, "preferences/why.md"));
    let signing = owned(&mine, &mine, None);
    (signing.put_file(&path("/preferences/why.md"), Expect::Any, bytes)).unwrap();
    assert!(exists(&mine, "preferences/why.md"));
}

#[test]
fn boards_own_writes_of_a_pending_effect_and_an_answer_need_no_owner_signature() {
    let (scenario, mine) = (board(), personal());
    let (_tools, decline, _keep) = owner_signers();
    setting(&scenario, "tasks", "/tasks/*", "ask");
    let mut plugin = asker(Some("tasks"), Some(&mine));
    plugin.sign_owner_writes = true;
    plugin.signing.signer = Some(decline.clone().into_os_string());
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.ask_as(plugin);
    let id = asked(put(&board, "/tasks/a", &task("")));
    assert!(exists(&mine, &format!("effects/pending/{id}.json")));
    let mut owner = asker(None, Some(&mine));
    owner.sign_owner_writes = true;
    owner.signing.signer = Some(decline.into_os_string());
    answer(&owner, &id, Answer::Refuse, None, false).unwrap();
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
}

#[test]
fn the_owner_signs_a_put_under_settings_preferences_or_effects_in_any_case_or_a_first_directory_that_is_not_ascii()
 {
    for (given, on_disk, signs) in [
        (".board/settings/r.json", ".board/settings/r.json", true),
        (".BOARD/Settings/r.json", ".BOARD/Settings/r.json", true),
        (
            ".board/types/a.schema.json",
            ".board/types/a.schema.json",
            false,
        ),
        (".board/board.json", ".board/board.json", false),
        (
            ".board/s\u{0131}ttings/r.json",
            ".board/s\u{0131}ttings/r.json",
            true,
        ),
        ("preferences/r.json", "preferences/r.json", true),
        ("effects/pending/e.json", "effects/pending/e.json", true),
        ("pref/r.json", "preferences/r.json", true),
        ("preferences.json", "preferences.json", false),
        ("tasks/a.json", "tasks/a.json", false),
        ("preferenc\u{017f}/r.json", "preferenc\u{017f}/r.json", true),
        ("\u{00f1}.json", "\u{00f1}.json", false),
    ] {
        assert_eq!(
            super::owner_signs(given, on_disk),
            signs,
            "{given} at {on_disk}"
        );
    }
}

proptest::proptest! {
    #[test]
    fn an_ascii_path_outside_settings_preferences_and_effects_needs_no_owner_signature(
        first in "[a-z0-9._-]{1,12}",
        rest in "[a-z0-9._/-]{0,20}",
    ) {
        proptest::prop_assume!(!matches!(first.as_str(), ".board" | "preferences" | "effects"));
        let file = format!("{first}/{rest}");
        proptest::prop_assert!(!super::owner_signs(&file, &file));
    }
}

#[test]
fn a_plugins_put_under_preferences_or_effects_is_refused_in_a_board_that_is_not_the_personal_one_and_a_put_elsewhere_acts()
 {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let rule = preference("/tasks/*", "act");
    let file = board.put_file(&path("/preferences/w.md"), Expect::Any, b"text\n");
    assert_eq!(effect(file).unwrap_err().code(), Code::SettingsFromPlugin);
    for at in ["/Preferences/w", "/effects/seen/s"] {
        assert_eq!(
            code(put(&board, at, &rule)),
            Code::SettingsFromPlugin,
            "{at}"
        );
    }
    assert!(!exists(&scenario, "preferences/w.json"));
    put(&board, "/tasks/a", &task("")).unwrap();
    assert!(exists(&scenario, "tasks/a.json"));
}

#[test]
fn a_plugins_removal_of_a_setting_or_a_link_is_refused_and_the_owners_and_one_elsewhere_are_admitted()
 {
    let scenario = board();
    scenario
        .write(
            ".board/settings/a.json",
            Item::File(&preference("/*", "act")),
        )
        .write("notes/l.json", Item::File(&link("/projects/p")));
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    for at in ["/.board/settings/a", "/notes/l"] {
        let refused = rm(&plugin, at).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        assert!(refused.to_string().contains("remove"), "{refused}");
    }
    rm(&owner, "/notes/l").unwrap();
    rm(&owner, "/.board/settings/a").unwrap();
    rm(&plugin, "/projects/p").unwrap();
    assert!(!exists(&scenario, "projects/p.json"));
}

#[test]
fn a_removal_a_rule_asks_for_holds_a_pending_rm_until_approved_and_a_put_rule_does_not_cover_it() {
    let (scenario, mine) = (board(), personal());
    scenario.write("tasks/a.json", Item::File(&task("")));
    setting(&scenario, "puts", "/tasks/*", "ask");
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    rm(&board, "/tasks/a").unwrap();
    scenario.write("tasks/a.json", Item::File(&task(""))).write(
        ".board/settings/rm.json",
        Item::File(&rule("rm", "/tasks/*", "ask")),
    );
    let id = asked(rm(&board, "/tasks/a"));
    let effect = read(&mine, &format!("effects/pending/{id}.json"));
    assert_eq!(effect["op"], "rm");
    assert_eq!(effect["path"], "/tasks/a");
    assert!(effect["prior"].is_string(), "{effect}");
    assert!(
        effect.get("record").is_none() && effect.get("digest").is_none(),
        "{effect}"
    );
    assert!(exists(&scenario, "tasks/a.json"));
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, false).unwrap();
    rm(&board, "/tasks/a").unwrap();
    assert!(!exists(&scenario, "tasks/a.json"));
}

#[test]
fn an_owners_removal_of_a_rule_is_refused_unless_they_sign_it_and_one_elsewhere_needs_no_signature()
{
    let mine = personal();
    mine.write("preferences/r.json", Item::File(&preference("/*", "act")))
        .write("tasks/a.json", Item::File(&task("")));
    let (tools, decline, keep) = owner_signers();
    let declining = owned(&mine, &mine, Some(&decline));
    assert_eq!(
        code(rm(&declining, "/preferences/r")),
        Code::SettingsUnsigned
    );
    assert!(exists(&mine, "preferences/r.json"));
    rm(&declining, "/tasks/a").unwrap();
    rm(&owned(&mine, &mine, Some(&keep)), "/preferences/r").unwrap();
    assert!(!exists(&mine, "preferences/r.json"));
    let shown = std::fs::read_to_string(tools.workdir().join("shown")).unwrap();
    let root = mine.workdir().canonicalize().unwrap();
    assert_eq!(
        shown,
        format!(
            "board/write/v1\nop: rm\npath: /preferences/r\nboard: {}\n",
            root.display()
        )
    );
}

#[test]
fn deciding_an_rm_holds_no_pending_effect_and_approving_one_with_save_rule_is_refused_as_cannot_widen()
 {
    let (scenario, mine) = (board(), personal());
    scenario
        .write("tasks/a.json", Item::File(&task("")))
        .write(
            ".board/settings/rm.json",
            Item::File(&rule("rm", "/tasks/*", "ask")),
        )
        .write(
            ".board/settings/comment.json",
            Item::File(&rule("review.comment", "/tasks/*", "ask")),
        );
    let board = opened(&scenario, Some("tasks"), Some(&mine));
    let decided = board.authority("rm", &path("/tasks/a")).unwrap();
    assert_eq!(
        (decided.decision.authority, decided.effect),
        (Authority::Ask, None)
    );
    assert!(!exists(&mine, "effects"));
    let comment = board
        .authority("review.comment", &path("/tasks/a"))
        .unwrap();
    assert!(comment.effect.is_some());
    let id = asked(rm(&board, "/tasks/a"));
    let owner = asker(None, Some(&mine));
    let widen = answer(&owner, &id, Answer::Approve, None, true);
    assert_eq!(code(widen), Code::SettingsCannotWiden);
    let saved = answer(&owner, &id, Answer::Refuse, None, true).unwrap();
    assert_eq!(saved.rule, Some(format!("/preferences/{id}")));
}

#[test]
fn an_owners_removal_of_the_board_config_is_refused_unless_they_sign_it_and_a_put_of_it_needs_no_signature()
 {
    let mine = personal();
    let (_tools, decline, keep) = owner_signers();
    let declining = owned(&mine, &mine, Some(&decline));
    let refused = rm(&declining, "/.board/board").unwrap_err();
    assert_eq!(refused.code(), Code::SettingsUnsigned);
    let said = refused.to_string();
    assert!(
        said.starts_with(
            "the owner's rm of /.board/board changes a rule, an effect or the board's config"
        ),
        "{said}"
    );
    assert!(exists(&mine, ".board/board.json"));
    let config = std::fs::read_to_string(mine.workdir().join(".board/board.json")).unwrap();
    put(&declining, "/.board/board", &config).unwrap();
    rm(&owned(&mine, &mine, Some(&keep)), "/.board/board").unwrap();
    assert!(!exists(&mine, ".board/board.json"));
}
