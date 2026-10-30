//! What only the owner writes, `decide` and `answer`, each refusal beside an
//! admitted twin.

use std::path::{Path, PathBuf};

use board_model::{
    APPROVAL_HEADER, Answer, Code, Digest, Effect, LogicalPath, Seen, WRITE_HEADER, canonical_json,
};
use board_scenario::seed::mount_of;
use board_scenario::signer::Signer;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{Asker, DecideReport, EffectError, Shown, Signing, answer, hold};
use crate::Board;
use crate::put::{Expect, PutError, PutReport, Removing, Unmatched};
use crate::tests::{TASK_SCHEMA, task};

const CONFIG: &str = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;

/// The op the tests ask for.
const OP: &str = "review.comment";

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
        plugin: plugin.map(|plugin| plugin.parse().unwrap()),
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

/// `board decide --op review.comment --path <at>`.
fn decide(board: &Board, at: &str) -> Result<DecideReport, EffectError> {
    board.decide_op(OP, &path(at), None)
}

/// [`decide`] showing `shown`, as `--text` or `--record` gives it.
fn decide_with(board: &Board, at: &str, shown: &Shown) -> Result<DecideReport, EffectError> {
    board.decide_op(OP, &path(at), Some(shown))
}

fn text(text: &str) -> Shown {
    Shown::Text(text.as_bytes().to_vec())
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
fn asked(result: Result<DecideReport, EffectError>) -> String {
    match result.unwrap_err() {
        EffectError::Asked { id, .. } => id,
        other => panic!("not asked: {other}"),
    }
}

/// The time `offset` seconds from now, as Board writes `asked`.
fn now(offset: i64) -> String {
    let now = std::time::SystemTime::now();
    let shift = std::time::Duration::from_secs(offset.unsigned_abs());
    let shifted = if offset < 0 { now - shift } else { now + shift };
    board_model::utc(shifted)
}

fn link(to: &str) -> String {
    format!(r#"{{"$type": "board/mount/v1", "path": "{to}"}}"#)
}

#[test]
fn a_plugins_decide_with_no_answer_writes_a_pending_effect_and_is_refused_as_asked_and_the_owners_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let owner = opened(&scenario, None, None);
    let refused = decide(&plugin, "/pulls/7").unwrap_err();
    assert_eq!(refused.code(), Code::SettingsAsked);
    assert_eq!(
        refused.to_string(),
        format!(
            "plugin review may not review.comment /pulls/7 until the owner answers pending effect {}",
            refused.fields().effect.unwrap()
        )
    );
    let id = refused.fields().effect.unwrap();
    let mut effect = read(&mine, &format!("effects/pending/{id}.json"));
    let asked_at = effect.as_object_mut().unwrap().remove("asked").unwrap();
    let asked_at = asked_at.as_str().unwrap();
    let (before, after) = (now(-60), now(60));
    assert!(
        before.as_str() < asked_at && asked_at < after.as_str(),
        "{asked_at}"
    );
    let root = plugin.repository().path().display().to_string();
    assert_eq!(
        effect,
        json!({"$type": "board/effect/v1", "board": root, "plugin": "review", "op": OP, "path": "/pulls/7"})
    );
    // Asking again finds the same pending effect and keeps when it was asked.
    let held = std::fs::read(mine.workdir().join(format!("effects/pending/{id}.json"))).unwrap();
    assert_eq!(asked(decide(&plugin, "/pulls/7")), id);
    let again = std::fs::read(mine.workdir().join(format!("effects/pending/{id}.json"))).unwrap();
    assert_eq!(held, again);
    let admitted = decide(&owner, "/pulls/7").unwrap();
    assert_eq!(admitted.effect, None);
}

#[test]
fn an_approved_effect_admits_the_same_decide_and_a_refused_one_is_refused_naming_it() {
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let approved = asked(decide(&plugin, "/pulls/7"));
    answer(&owner, &approved, Answer::Approve, None, None).unwrap();
    let report = decide(&plugin, "/pulls/7").unwrap();
    assert_eq!(report.effect.as_deref(), Some(approved.as_str()));
    let refused = asked(decide(&plugin, "/pulls/8"));
    answer(&owner, &refused, Answer::Refuse, None, None).unwrap();
    let error = decide(&plugin, "/pulls/8").unwrap_err();
    assert_eq!(error.code(), Code::SettingsRefused);
    assert_eq!(error.fields().effect.as_deref(), Some(refused.as_str()));
    let hint = error.refusal().hint.unwrap();
    assert!(
        hint.contains(&format!("`board answer {refused} approve`")),
        "{hint}"
    );
    // Another plugin asking for the approved op is another request.
    let other = opened(&scenario, Some("notes"), Some(&mine));
    assert_ne!(asked(decide(&other, "/pulls/7")), approved);
}

#[test]
fn a_plugins_decide_with_no_personal_board_is_refused_as_no_personal_and_the_owners_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("review"), None);
    let refused = decide(&plugin, "/pulls/7").unwrap_err();
    assert_eq!(refused.code(), Code::SettingsNoPersonal);
    assert_eq!(
        refused.to_string(),
        "review.comment /pulls/7 needs a pending effect in the personal board, but the known boards file names no personal board"
    );
    decide(&opened(&scenario, None, None), "/pulls/7").unwrap();
}

/// A board in its own repository that commits its puts, with a task schema
/// and an identity, and the test board mounting it at `/m`.
fn mounted() -> (Scenario, Scenario) {
    let library = Scenario::new();
    let files = [
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
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
fn a_decide_whose_landing_has_no_logical_path_is_refused_as_malformed_and_one_that_has_is_named() {
    let asked = path("/alias");
    let refused = super::decide::landed(&asked, "/a\u{202e}b".parse()).unwrap_err();
    assert_eq!(refused.code(), Code::PathMalformed);
    assert_eq!(refused.fields().path, Some(asked.clone()));
    let admitted = super::decide::landed(&asked, "/ab".parse()).unwrap();
    assert_eq!(admitted, path("/ab"));
}

#[test]
fn deciding_through_a_link_or_a_mount_names_where_a_put_would_land_and_its_twin_names_the_path() {
    let ((scenario, library), mine) = (mounted(), personal());
    scenario
        .write("l.json", Item::File(&link("/pulls")))
        .write("pulls/b.json", Item::File(&task("")));
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let effect = |at: &str| {
        read(
            &mine,
            &format!("effects/pending/{}.json", asked(decide(&plugin, at))),
        )
    };
    let root = plugin.repository().path().display().to_string();
    let inner = library
        .workdir()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    let linked = effect("/l/7");
    assert_eq!(
        (linked["board"].as_str(), linked["path"].as_str()),
        (Some(&*root), Some("/pulls/7"))
    );
    let through = effect("/m/notes/y");
    assert_eq!(
        (through["board"].as_str(), through["path"].as_str()),
        (Some(&*inner), Some("/notes/y"))
    );
    let plain = effect("/k/7");
    assert_eq!(
        (plain["board"].as_str(), plain["path"].as_str()),
        (Some(&*root), Some("/k/7"))
    );
    // Text shown through the mount reaches the mounted board's effect.
    let id = asked(decide_with(&plugin, "/m/notes/y", &text("t")));
    let shown = read(&mine, &format!("effects/pending/{id}.json"));
    assert_eq!(
        (shown["board"].as_str(), shown["text"].as_str()),
        (Some(&*inner), Some("t"))
    );
    assert_eq!(shown["digest"], Digest::of(b"t").to_string());
    assert_ne!(id, asked(decide(&plugin, "/m/notes/y")));
}

#[test]
fn a_decide_showing_other_text_at_the_same_path_is_another_pending_effect_and_the_same_text_is_the_same_one()
 {
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let a = asked(decide_with(&plugin, "/pulls/7", &text("a")));
    let b = asked(decide_with(&plugin, "/pulls/7", &text("b")));
    assert_ne!(a, b);
    assert_eq!(asked(decide_with(&plugin, "/pulls/7", &text("a"))), a);
    let shown = pending_effect(&mine, &a);
    assert_eq!(
        (shown.text.as_deref(), shown.digest),
        (Some("a"), Some(Digest::of(b"a")))
    );
    // The earlier pending effect stays open.
    assert!(exists(&mine, &format!("effects/pending/{b}.json")));
}

#[test]
fn an_approval_of_shown_text_admits_only_that_text_and_moved_to_other_text_is_refused_as_unsigned()
{
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let a = asked(decide_with(&plugin, "/pulls/7", &text("a")));
    answer(&owner, &a, Answer::Approve, None, None).unwrap();
    let report = decide_with(&plugin, "/pulls/7", &text("a")).unwrap();
    assert_eq!(report.effect.as_deref(), Some(a.as_str()));
    let b = asked(decide_with(&plugin, "/pulls/7", &text("b")));
    assert_ne!(b, a);
    // A bare decide names no content, so it is another request.
    assert_ne!(asked(decide(&plugin, "/pulls/7")), a);
    // A's approval at B's id signs A's statement, which is not B's.
    let seen = read(&mine, &format!("effects/seen/{a}.json"));
    mine.write(
        &format!("effects/seen/{b}.json"),
        Item::File(&seen.to_string()),
    );
    let moved = decide_with(&plugin, "/pulls/7", &text("b"));
    assert_eq!(named(moved), (Code::SettingsUnsigned, Some(b)));
}

#[test]
fn an_approval_of_a_shown_record_admits_it_and_its_digest_is_that_of_its_canonical_json() {
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("review"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let record = Shown::Record(br#"{"b": 1, "a": 2}"#.to_vec());
    let id = asked(decide_with(&plugin, "/pulls/7", &record));
    let shown = pending_effect(&mine, &id);
    assert_eq!(shown.record, Some(json!({"a": 2, "b": 1})));
    let canonical = canonical_json(shown.record.as_ref().unwrap());
    assert_eq!(shown.digest, Some(Digest::of(canonical.as_bytes())));
    answer(&owner, &id, Answer::Approve, None, None).unwrap();
    let report = decide_with(&plugin, "/pulls/7", &record).unwrap();
    assert_eq!(report.effect, Some(id.clone()));
    // Its canonical JSON as text has the same id, so the record's approval
    // is found and does not verify for the text's statement.
    let as_text = decide_with(&plugin, "/pulls/7", &text(canonical.as_str()));
    assert_eq!(named(as_text), (Code::SettingsUnsigned, Some(id)));
}

#[test]
fn content_over_the_limit_is_refused_as_too_large_and_content_at_it_is_admitted() {
    let shows = |shown: &Shown| super::decide::shows(Some(shown), 4);
    for over in [text("abcde"), Shown::Record(b"{ } \n".to_vec())] {
        assert_eq!(code(shows(&over)), Code::JsonTooLarge);
    }
    assert_eq!(
        shows(&text("abcd")).unwrap(),
        (None, Some("abcd".to_owned()))
    );
    assert_eq!(
        shows(&Shown::Record(b"{  }".to_vec())).unwrap(),
        (Some(json!({})), None)
    );
    let refused = shows(&text("abcde")).unwrap_err().refusal();
    assert_eq!(
        refused.message,
        "the text to decide on is over the limit of 4 bytes"
    );
    assert_eq!(super::decide::shows(None, 0).unwrap(), (None, None));
}

#[test]
fn a_record_whose_canonical_json_is_over_the_limit_is_refused_though_its_input_is_under_it_and_a_smaller_one_is_admitted()
 {
    let shows = |input: &str| super::decide::shows(Some(&Shown::Record(input.into())), 20);
    // 11 bytes in, 37 bytes pretty-printed.
    let nested = shows(r#"{"a":[[1]]}"#).unwrap_err();
    assert_eq!(nested.code(), Code::JsonTooLarge);
    assert_eq!(
        nested.to_string(),
        "the record to decide on is over the limit of 20 bytes"
    );
    // 7 bytes in, 13 bytes pretty-printed.
    assert_eq!(shows(r#"{"a":1}"#).unwrap(), (Some(json!({"a": 1})), None));
}

#[test]
fn an_answer_to_a_pending_effect_that_shows_a_record_and_text_without_a_digest_is_refused_and_the_record_alone_is_admitted()
 {
    let mine = personal();
    let owner = asker(None, Some(&mine));
    let record = json!({"a": 1});
    let effect = Effect::new(
        "/b".to_owned(),
        "review".parse().unwrap(),
        OP.to_owned(),
        "/pulls/7".to_owned(),
    );
    let mut both = effect.clone().showing(Some(record.clone()), None);
    (both.text, both.digest) = (Some("t".to_owned()), None);
    pending(&mine, &both.id(), &both);
    let refused = answer(&owner, &both.id(), Answer::Approve, None, None).unwrap_err();
    assert_eq!(refused.code(), Code::SettingsNotFound);
    assert!(refused.to_string().contains("its digest"), "{refused}");
    let alone = effect.showing(Some(record), None);
    pending(&mine, &alone.id(), &alone);
    answer(&owner, &alone.id(), Answer::Approve, None, None).unwrap();
}

#[test]
fn a_record_that_repeats_a_key_or_holds_a_lossy_number_says_so_with_its_code_and_its_twin_is_admitted()
 {
    let shows = |input: &str| super::decide::shows(Some(&Shown::Record(input.into())), 64);
    let repeated = shows(r#"{"a":1,"a":2}"#).unwrap_err();
    assert_eq!(repeated.code(), Code::JsonDuplicateKey);
    assert_eq!(
        repeated.to_string(),
        r#"the record to decide on is refused: json repeats the key "a" in one object"#
    );
    let lossy = shows(r#"{"a":123456789012345678901}"#).unwrap_err();
    assert_eq!(lossy.code(), Code::JsonLossyNumber);
    assert!(
        lossy.to_string().contains("cannot be held exactly"),
        "{lossy}"
    );
    shows(r#"{"a":1,"b":1e4}"#).unwrap();
}

#[test]
fn content_that_is_not_utf8_or_not_a_json_object_is_refused_with_its_code_and_its_twin_is_admitted()
{
    let scenario = board();
    let owner = opened(&scenario, None, None);
    let decided = |shown: Shown| decide_with(&owner, "/pulls/7", &shown);
    let record = |text: &str| Shown::Record(text.as_bytes().to_vec());
    let cases = [
        (Shown::Text(vec![b'a', 0xff]), Code::JsonNotUtf8, text("aÿ")),
        (record("{"), Code::JsonParse, record("{}")),
        (
            record(r#"{"a":1,"a":2}"#),
            Code::JsonDuplicateKey,
            record(r#"{"a":1}"#),
        ),
        (record("[]"), Code::RecordNotARecord, record("{}")),
        (record(r#""a""#), Code::RecordNotARecord, text(r#""a""#)),
    ];
    for (refused, refused_code, admitted) in cases {
        assert_eq!(code(decided(refused)), refused_code);
        decided(admitted).unwrap();
    }
}

#[test]
fn an_answer_to_a_pending_effect_that_shows_text_without_its_digest_is_refused_and_one_with_it_is_admitted()
 {
    let mine = personal();
    let owner = asker(None, Some(&mine));
    let effect = Effect::new(
        "/b".to_owned(),
        "review".parse().unwrap(),
        OP.to_owned(),
        "/pulls/7".to_owned(),
    );
    let mut bare = effect.clone().showing(None, Some("t".to_owned()));
    bare.digest = None;
    pending(&mine, &bare.id(), &bare);
    let refused = answer(&owner, &bare.id(), Answer::Approve, None, None).unwrap_err();
    assert_eq!(refused.code(), Code::SettingsNotFound);
    assert!(refused.to_string().contains("its digest"), "{refused}");
    let digested = effect.showing(None, Some("t".to_owned()));
    pending(&mine, &digested.id(), &digested);
    answer(&owner, &digested.id(), Answer::Approve, None, None).unwrap();
}

#[test]
fn a_plugins_put_under_dot_board_is_refused_whatever_its_type_and_a_task_elsewhere_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    let project = r#"{"$type": "lr/project/v1"}"#;
    for record in [project, &task("")] {
        for at in ["/.board/notes/a", "/.board/Notes/b"] {
            let refused = put(&plugin, at, record);
            assert_eq!(code(refused), Code::SettingsFromPlugin, "{at}");
        }
    }
    put(&owner, "/.board/notes/a", project).unwrap();
    put(&plugin, "/notes/a", &task("")).unwrap();
    let file = |at: &str| effect(plugin.put_file(&path(at), Expect::Any, b"x"));
    assert_eq!(code(file("/.board/notes/a.md")), Code::SettingsFromPlugin);
    file("/notes/a.md").unwrap();
    let config = put(&plugin, "/.board/board", r#"{"$type": "board/config/v1"}"#);
    assert_eq!(code(config), Code::SettingsFromPlugin);
}

#[test]
fn a_plugins_put_under_effects_is_refused_in_the_personal_board_and_another_and_one_under_preferences_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    for board in [&mine, &scenario] {
        let plugin = opened(board, Some("tasks"), Some(&mine));
        for at in [
            "/effects/pending/x",
            "/effects/seen/x",
            "/effects/answered/x",
            "/Effects/x",
        ] {
            let refused = put(&plugin, at, &task("")).unwrap_err();
            assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        }
        let file = plugin.put_file(&path("/effects/w.md"), Expect::Any, b"text\n");
        assert_eq!(effect(file).unwrap_err().code(), Code::SettingsFromPlugin);
        put(&plugin, "/preferences/x", &task("")).unwrap();
        assert!(exists(board, "preferences/x.json"));
    }
    assert!(!exists(&mine, "effects"));
}

#[test]
fn a_plugins_put_of_an_effect_or_seen_record_is_refused_in_any_board_and_the_owners_is_admitted() {
    let (scenario, mine) = (board(), personal());
    let effect = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        OP.to_owned(),
        "/x".to_owned(),
    );
    let seen = json!({"$type": "board/seen/v1", "effect": "0".repeat(64), "answer": "approve"});
    let records = [serde_json::to_string(&effect).unwrap(), seen.to_string()];
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    let owner = opened(&scenario, None, Some(&mine));
    for (at, record) in ["/notes/a", "/notes/b"].iter().zip(&records) {
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
fn a_plugins_put_through_a_link_into_a_protected_file_is_refused_and_through_another_link_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    let project = r#"{"$type": "lr/project/v1"}"#;
    scenario
        .write(".board/notes/strict.json", Item::File(project))
        .write(
            "notes/strict.json",
            Item::File(&link("/.board/notes/strict")),
        )
        .write("notes/project.json", Item::File(&link("/projects/p")));
    let plugin = opened(&scenario, Some("tasks"), None);
    let refused = put(&plugin, "/notes/strict", project);
    assert_eq!(code(refused), Code::SettingsFromPlugin);
    put(&plugin, "/notes/project", project).unwrap();
    mine.write("effects/x.json", Item::File(&task("")))
        .write("notes/x.json", Item::File(&link("/effects/x")));
    let inside = opened(&mine, Some("tasks"), Some(&mine));
    assert_eq!(
        code(put(&inside, "/notes/x", &task(""))),
        Code::SettingsFromPlugin
    );
}

#[test]
fn a_plugins_put_of_a_link_is_refused_and_the_owners_is_admitted() {
    let scenario = board();
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    let to = link("/projects/p");
    assert_eq!(
        code(put(&plugin, "/notes/l", &to)),
        Code::SettingsFromPlugin
    );
    put(&owner, "/notes/l", &to).unwrap();
}

#[test]
fn a_plugins_put_through_a_mount_is_refused_where_the_mounted_board_is_protected_and_admitted_elsewhere()
 {
    let (scenario, library) = mounted();
    let plugin = opened(&scenario, Some("tasks"), None);
    let inner = put(&plugin, "/m/.board/notes/a", &task(""));
    assert_eq!(code(inner), Code::SettingsFromPlugin);
    assert_eq!(
        code(put(&plugin, "/m/effects/z", &task(""))),
        Code::SettingsFromPlugin
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
    let refused = effect(plugin.put_file(&path("/m/effects/z"), Expect::Any, bytes));
    assert_eq!(code(refused), Code::SettingsFromPlugin);
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
fn a_batch_with_a_line_only_the_owner_writes_is_refused_naming_the_line_and_its_twin_is_admitted() {
    let scenario = board();
    let board = opened(&scenario, Some("tasks"), None);
    let batch = |at: &str| format!("{}\n{}\n", line("/tasks/c"), line(at));
    let refused = board
        .put_batch(&mut batch("/effects/a").as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutInvalid);
    assert_eq!(lines(&refused), [(2, Code::SettingsFromPlugin)]);
    assert!(!exists(&scenario, "tasks/c.json"));
    board.put_batch(&mut batch("/tasks/b").as_bytes()).unwrap();
    assert!(exists(&scenario, "tasks/c.json"));
}

#[test]
fn an_answer_moves_its_pending_effect_to_answered_in_the_commit_that_writes_the_seen_record_and_an_unanswered_one_stays()
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
    let board = opened(&scenario, Some("review"), Some(&mine));
    let answered = asked(decide(&board, "/pulls/7"));
    let unanswered = asked(decide(&board, "/pulls/8"));
    let pending = |id: &str| format!("effects/pending/{id}.json");
    let asked_bytes = std::fs::read(mine.workdir().join(pending(&answered))).unwrap();
    let before = mine.head().unwrap();
    let owner = asker(None, Some(&mine));
    let report = answer(&owner, &answered, Answer::Approve, None, None).unwrap();
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
    let kept = format!("effects/answered/{answered}.json");
    assert_eq!(head.files.get(&kept), Some(&asked_bytes));
    assert!(!exists(
        &mine,
        &format!("effects/answered/{unanswered}.json")
    ));
    assert!(head.files.contains_key(&pending(&unanswered)));
    assert!(exists(&mine, &pending(&unanswered)));
    // The approved op finds the seen record by its id and is admitted.
    decide(&board, "/pulls/7").unwrap();
}

/// The owner removes an answer, the plugin asks again, and the owner
/// answers again: the second answer keeps the effect as it was asked the
/// second time.
#[test]
fn an_effect_asked_again_after_its_answer_is_removed_replaces_the_answered_copy_when_answered() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answer(&owner, &id, Answer::Refuse, None, None).unwrap();
    let kept = format!("effects/answered/{id}.json");
    let mut first = read(&mine, &kept);
    first["asked"] = json!("1970-01-01T00:00:00.000000Z");
    mine.write(&kept, Item::File(&first.to_string()))
        .write(&format!("effects/seen/{id}.json"), Item::Deleted);
    assert_eq!(asked(decide(&board, "/pulls/7")), id);
    let again = read(&mine, &format!("effects/pending/{id}.json"));
    answer(&owner, &id, Answer::Refuse, None, None).unwrap();
    assert_eq!(read(&mine, &kept), again);
    assert_ne!(again, first);
}

#[test]
fn a_second_answer_to_the_same_effect_is_refused_as_not_found_and_the_first_is_admitted() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let owner = asker(None, Some(&mine));
    answer(
        &owner,
        &id,
        Answer::Refuse,
        Some("not now".to_owned()),
        None,
    )
    .unwrap();
    let again = answer(&owner, &id, Answer::Approve, None, None);
    assert_eq!(code(again), Code::SettingsNotFound);
    let seen = read(&mine, &format!("effects/seen/{id}.json"));
    assert_eq!(
        (seen["answer"].as_str(), seen["reason"].as_str()),
        (Some("refuse"), Some("not now"))
    );
}

/// A decide reads no answer, the owner refuses, and then the decide holds
/// its effect: the hold writes nothing, and a pending effect left over from
/// before cannot turn the refusal into an approval.
#[test]
fn a_hold_after_the_owner_answers_writes_no_pending_effect_and_a_hold_before_writes_one() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let owner = asker(None, Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let file = format!("effects/pending/{id}.json");
    let text = std::fs::read_to_string(mine.workdir().join(&file)).unwrap();
    let effect = Effect::from_value(&serde_json::from_str(&text).unwrap()).unwrap();
    let personal = Board::open(mine.workdir(), None).unwrap();
    answer(&owner, &id, Answer::Refuse, None, None).unwrap();
    hold(&personal, &effect).unwrap();
    assert!(!exists(&mine, &file));
    mine.write(&file, Item::File(&text));
    let again = answer(&owner, &id, Answer::Approve, None, None);
    assert_eq!(code(again), Code::PutStale);
    assert_eq!(
        read(&mine, &format!("effects/seen/{id}.json"))["answer"],
        "refuse"
    );
    let unanswered = asked(decide(&board, "/pulls/8"));
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
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let owner = asker(None, Some(&mine));
    let approve = |asker: &Asker, id: &str| answer(asker, id, Answer::Approve, None, None);
    let plugin = asker(Some("review"), Some(&mine));
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
    approve(&owner, &id).unwrap();
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
}

/// Writes `effect` in `mine` as the pending effect `id`.
fn pending(mine: &Scenario, id: &str, effect: &Effect) {
    let value = serde_json::to_string(effect).unwrap();
    mine.write(&format!("effects/pending/{id}.json"), Item::File(&value));
}

#[test]
fn an_answer_to_a_pending_effect_that_is_not_the_one_its_id_names_is_refused_and_the_true_one_is_admitted()
 {
    let mine = personal();
    let owner = asker(None, Some(&mine));
    let approve = |id: &str| answer(&owner, id, Answer::Approve, None, None);
    let mut effect = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "put".to_owned(),
        "/x".to_owned(),
    );
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
fn a_plugins_put_whose_path_is_not_ascii_is_refused_and_its_ascii_twin_is_admitted() {
    let (scenario, mine) = (board(), personal());
    let plugin = opened(&scenario, Some("tasks"), Some(&mine));
    // `ſ` folds to `s` and the Kelvin sign to `k` on a case-insensitive
    // file system, so each would land on a protected file.
    for at in ["/.board/ſettings/x", "/effectſ/seen/y", "/\u{212A}ept/x"] {
        let refused = put(&plugin, at, &task("")).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        assert!(refused.to_string().contains("not ASCII"), "{refused}");
    }
    put(&plugin, "/kept/x", &task("")).unwrap();
}

#[test]
fn a_plugins_put_in_another_ascii_case_of_an_existing_directory_is_refused_as_a_case_collision_and_its_twin_is_admitted()
 {
    let scenario = board();
    scenario.write("effects/a.json", Item::File(&task("")));
    let plugin = opened(&scenario, Some("tasks"), None);
    // A new directory differing only in case is refused before the check.
    let collides = |at: &str| plugin.put(&path(at), Expect::Any, task("").as_bytes());
    assert_eq!(
        collides("/EfFeCtS/b").unwrap_err().code(),
        Code::PathCaseCollision
    );
    put(&plugin, "/open/b", &task("")).unwrap();
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

/// The pending effect `id` in `mine`.
fn pending_effect(mine: &Scenario, id: &str) -> Effect {
    Effect::from_value(&read(mine, &format!("effects/pending/{id}.json"))).unwrap()
}

/// An approval of `id` written straight into `mine`, as an agent with the
/// owner's shell could write it, carrying `signature`.
fn approve_by_hand(mine: &Scenario, id: &str, signature: Option<String>) {
    let mut seen = Seen::new(format!("/effects/answered/{id}"), Answer::Approve, None);
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
#[expect(
    clippy::disallowed_methods,
    reason = "it signs the approval with the test key, as the signer would"
)]
fn an_approval_without_the_owners_signature_or_by_another_key_is_refused_as_unsigned_and_the_owners_is_admitted()
 {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let approval = pending_effect(&mine, &id).approval();
    let unsigned = (Code::SettingsUnsigned, Some(id.clone()));
    approve_by_hand(&mine, &id, None);
    assert_eq!(named(decide(&board, "/pulls/7")), unsigned);
    approve_by_hand(&mine, &id, Some(Signer::new().sign(&approval)));
    assert_eq!(named(decide(&board, "/pulls/7")), unsigned);
    let mut moved = pending_effect(&mine, &id);
    moved.path = "/pulls/8".to_owned();
    approve_by_hand(&mine, &id, Some(signer().sign(&moved.approval())));
    assert_eq!(named(decide(&board, "/pulls/7")), unsigned);
    approve_by_hand(&mine, &id, Some(signer().sign(&approval)));
    let report = decide(&board, "/pulls/7").unwrap();
    assert_eq!(report.effect, Some(id));
}

#[test]
fn approving_with_no_signer_is_refused_as_unsigned_and_writes_nothing_and_refusing_needs_no_signature()
 {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let unsigned_owner = Asker {
        signing: Signing::default(),
        ..asker(None, Some(&mine))
    };
    let id = asked(decide(&board, "/pulls/7"));
    assert_eq!(
        named(answer(&unsigned_owner, &id, Answer::Approve, None, None)),
        (Code::SettingsUnsigned, Some(id.clone()))
    );
    assert!(exists(&mine, &format!("effects/pending/{id}.json")));
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, None).unwrap();
    let seen = read(&mine, &format!("effects/seen/{id}.json"));
    let signature = seen["signature"].as_str().unwrap();
    let armor = signer().sign(&format!("{APPROVAL_HEADER}\n"));
    assert_eq!(signature.lines().next(), armor.lines().next());
    let refused = asked(decide(&board, "/pulls/8"));
    answer(&unsigned_owner, &refused, Answer::Refuse, None, None).unwrap();
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
    let board = opened(&scenario, Some("review"), Some(&mine));
    let mut declining = asker(None, Some(&mine));
    declining.signing.signer = Some(tools.workdir().join("decline").into_os_string());
    let id = asked(decide(&board, "/pulls/7"));
    let error = answer(&declining, &id, Answer::Approve, None, None).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(error.to_string().contains("wrong path"), "{error}");
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    answer(&asker(None, Some(&mine)), &id, Answer::Approve, None, None).unwrap();
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "it signs the approval with the test key, as the signer would"
)]
fn an_approval_with_a_signature_by_another_key_or_in_the_write_namespace_is_refused_as_unsigned_and_the_owners_is_admitted_without_the_signer()
 {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let approval = pending_effect(&mine, &id).approval();
    let mut no_signer = asker(None, Some(&mine));
    no_signer.signing.signer = None;
    let approve =
        |signature: String| answer(&no_signer, &id, Answer::Approve, None, Some(signature));
    let unsigned = (Code::SettingsUnsigned, Some(id.clone()));
    assert_eq!(named(approve(Signer::new().sign(&approval))), unsigned);
    let written = signer().sign_in(WRITE_HEADER, &approval);
    assert_eq!(named(approve(written)), unsigned);
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    let signature = signer().sign(&approval);
    approve(signature.clone()).unwrap();
    let mut expected = Seen::new(format!("/effects/answered/{id}"), Answer::Approve, None);
    expected.signature = Some(signature);
    let seen = Seen::from_value(&read(&mine, &format!("effects/seen/{id}.json"))).unwrap();
    assert_eq!(seen, expected);
    assert!(!exists(&mine, &format!("effects/pending/{id}.json")));
    assert_eq!(decide(&board, "/pulls/7").unwrap().effect, Some(id));
}

/// `scenario` opened by the owner, whose puts under `effects/` are signed
/// by `signer`, or by the test signer.
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
            Item::Executable("#!/bin/sh\necho 'not this write'\nexit 3\n"),
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
fn an_owners_put_under_effects_is_refused_unless_they_sign_it_and_a_put_elsewhere_needs_no_signature()
 {
    let (scenario, mine) = (board(), personal());
    let (tools, decline, keep) = owner_signers();
    let declining = owned(&mine, &mine, Some(&decline));
    for (at, file) in [
        ("/effects/seen/s", "effects/seen/s.json"),
        ("/effects/answered/a", "effects/answered/a.json"),
        ("/Effects/r", "Effects/r.json"),
    ] {
        let error = put(&declining, at, &task("")).unwrap_err();
        assert_eq!(error.code(), Code::SettingsUnsigned, "{at}");
        assert!(matches!(error, EffectError::OwnerUnsigned { .. }), "{at}");
        assert!(error.to_string().contains("not this write"), "{error}");
        assert!(!exists(&mine, file), "{at}");
    }
    for (at, file) in [
        ("/tasks/a", "tasks/a.json"),
        ("/preferences/r", "preferences/r.json"),
        ("/.board/settings/r", ".board/settings/r.json"),
    ] {
        put(&declining, at, &task("")).unwrap();
        assert!(exists(&mine, file), "{at}");
    }
    let other = owned(&scenario, &mine, Some(&decline));
    let error = put(&other, "/effects/r", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(!exists(&scenario, "effects/r.json"));
    let signing = owned(&scenario, &mine, Some(&keep));
    put(&signing, "/effects/r", &task("")).unwrap();
    assert!(exists(&scenario, "effects/r.json"));
    let shown = std::fs::read_to_string(tools.workdir().join("shown")).unwrap();
    let root = scenario.workdir().canonicalize().unwrap();
    assert!(
        shown.starts_with(&format!(
            "{WRITE_HEADER}\nop: put\npath: /effects/r\nboard: {}\ncontent: ",
            root.display()
        )),
        "{shown}"
    );
    assert!(shown.contains("\"title\": \"t\""), "{shown}");
}

#[test]
fn an_owners_put_under_effects_signed_as_an_approval_is_refused() {
    let mine = personal();
    let as_approval = owned(&mine, &mine, Some(&signer().program_in(APPROVAL_HEADER)));
    let error = put(&as_approval, "/effects/r", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned);
    assert!(!exists(&mine, "effects/r.json"));
    put(&owned(&mine, &mine, None), "/effects/r", &task("")).unwrap();
    assert!(exists(&mine, "effects/r.json"));
}

#[test]
fn an_owners_file_put_under_effects_needs_their_signature_and_a_signed_one_writes() {
    let mine = personal();
    let (_tools, decline, _keep) = owner_signers();
    let bytes = b"why the answers are as they are\n";
    let declining = owned(&mine, &mine, Some(&decline));
    let refused = declining.put_file(&path("/effects/why.md"), Expect::Any, bytes);
    assert_eq!(effect(refused).unwrap_err().code(), Code::SettingsUnsigned);
    assert!(!exists(&mine, "effects/why.md"));
    let signing = owned(&mine, &mine, None);
    (signing.put_file(&path("/effects/why.md"), Expect::Any, bytes)).unwrap();
    assert!(exists(&mine, "effects/why.md"));
}

#[test]
fn boards_own_writes_of_a_pending_effect_an_answer_and_an_answered_effect_need_no_owner_signature()
{
    let (scenario, mine) = (board(), personal());
    let (_tools, decline, _keep) = owner_signers();
    let mut plugin = asker(Some("review"), Some(&mine));
    plugin.sign_owner_writes = true;
    plugin.signing.signer = Some(decline.clone().into_os_string());
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.ask_as(plugin);
    let id = asked(decide(&board, "/pulls/7"));
    assert!(exists(&mine, &format!("effects/pending/{id}.json")));
    let mut owner = asker(None, Some(&mine));
    owner.sign_owner_writes = true;
    owner.signing.signer = Some(decline.into_os_string());
    answer(&owner, &id, Answer::Refuse, None, None).unwrap();
    assert!(exists(&mine, &format!("effects/seen/{id}.json")));
    assert!(exists(&mine, &format!("effects/answered/{id}.json")));
}

#[test]
fn the_owner_signs_a_put_under_effects_in_any_case_or_a_first_directory_that_is_not_ascii() {
    for (given, on_disk, signs) in [
        ("effects/pending/e.json", "effects/pending/e.json", true),
        ("EFFECTS/seen/s.json", "EFFECTS/seen/s.json", true),
        ("effects/answered/a.json", "effects/answered/a.json", true),
        ("eff/r.json", "effects/r.json", true),
        ("effect\u{017f}/r.json", "effect\u{017f}/r.json", true),
        ("effects.json", "effects.json", false),
        ("\u{00f1}.json", "\u{00f1}.json", false),
        (".board/settings/r.json", ".board/settings/r.json", false),
        ("preferences/r.json", "preferences/r.json", false),
        (".board/board.json", ".board/board.json", false),
        ("tasks/a.json", "tasks/a.json", false),
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
    fn an_ascii_path_outside_effects_needs_no_owner_signature(
        first in "[a-z0-9._-]{1,12}",
        rest in "[a-z0-9._/-]{0,20}",
    ) {
        proptest::prop_assume!(first != "effects");
        let file = format!("{first}/{rest}");
        proptest::prop_assert!(!super::owner_signs(&file, &file));
    }
}

#[test]
fn a_plugins_removal_under_dot_board_or_of_a_link_is_refused_and_the_owners_and_one_elsewhere_are_admitted()
 {
    let scenario = board();
    scenario
        .write(".board/notes/a.json", Item::File(&task("")))
        .write("notes/l.json", Item::File(&link("/projects/p")));
    let plugin = opened(&scenario, Some("tasks"), None);
    let owner = opened(&scenario, None, None);
    for at in ["/.board/notes/a", "/notes/l"] {
        let refused = rm(&plugin, at).unwrap_err();
        assert_eq!(refused.code(), Code::SettingsFromPlugin, "{at}");
        assert!(refused.to_string().contains("remove"), "{refused}");
    }
    rm(&owner, "/notes/l").unwrap();
    rm(&owner, "/.board/notes/a").unwrap();
    rm(&plugin, "/projects/p").unwrap();
    assert!(!exists(&scenario, "projects/p.json"));
}

#[test]
fn an_owners_removal_under_effects_is_refused_unless_they_sign_it_and_one_elsewhere_needs_no_signature()
 {
    let mine = personal();
    mine.write("effects/r.json", Item::File(&task("")))
        .write("tasks/a.json", Item::File(&task("")));
    let (tools, decline, keep) = owner_signers();
    let declining = owned(&mine, &mine, Some(&decline));
    assert_eq!(code(rm(&declining, "/effects/r")), Code::SettingsUnsigned);
    assert!(exists(&mine, "effects/r.json"));
    rm(&declining, "/tasks/a").unwrap();
    rm(&owned(&mine, &mine, Some(&keep)), "/effects/r").unwrap();
    assert!(!exists(&mine, "effects/r.json"));
    let shown = std::fs::read_to_string(tools.workdir().join("shown")).unwrap();
    let root = mine.workdir().canonicalize().unwrap();
    assert_eq!(
        shown,
        format!(
            "{WRITE_HEADER}\nop: rm\npath: /effects/r\nboard: {}\n",
            root.display()
        )
    );
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
        said.starts_with("the owner's rm of /.board/board changes an effect or the board's config"),
        "{said}"
    );
    assert!(exists(&mine, ".board/board.json"));
    let config = std::fs::read_to_string(mine.workdir().join(".board/board.json")).unwrap();
    put(&declining, "/.board/board", &config).unwrap();
    rm(&owned(&mine, &mine, Some(&keep)), "/.board/board").unwrap();
    assert!(!exists(&mine, ".board/board.json"));
}
