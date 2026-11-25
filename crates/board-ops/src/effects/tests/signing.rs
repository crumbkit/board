//! Optional signing: each act with signing off, on and listed, on and
//! unlisted, and configured with the allowed signers file missing.

use std::path::PathBuf;

use board_model::{Answer, Code, Seen};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{
    Asker, EffectError, Signing, answer, asked, asker, board, decide, exists, opened, personal,
    put, read, signer, signing,
};
use crate::Board;
use crate::tests::task;

/// The answer and question an approval in chat names.
const ANSWER: &str = "/projects/p/answers/q";

/// `mine`'s config with `signing` set to `setting`.
fn set(mine: &Scenario, setting: &Value) {
    let config = json!({"$type": "board/config/v1", "commit_on_put": false, "signing": setting});
    mine.write(".board/board.json", Item::File(&config.to_string()));
}

/// A signers file with no `owner` line, so signing is off.
fn no_owner() -> PathBuf {
    let file = signer().directory().join("no_owner");
    signer().write("no_owner", Item::File("ana ssh-ed25519 AAAA\n"));
    file
}

/// The owner, with signing off: no `owner` line and no signer.
fn off(mine: &Scenario) -> Asker {
    Asker {
        signing: Signing {
            signer: None,
            allowed: Some(no_owner()),
            ..signing()
        },
        ..asker(None, Some(mine))
    }
}

/// The owner, with the allowed signers file gone.
fn missing(mine: &Scenario) -> Asker {
    Asker {
        signing: Signing {
            allowed: Some(signer().directory().join("gone")),
            ..signing()
        },
        ..asker(None, Some(mine))
    }
}

/// Writes in `mine` a question naming `target` and the owner's answer
/// choosing `approve`, by `by`, at [`ANSWER`].
fn answered(mine: &Scenario, target: &str, by: &Value) {
    let question = json!({
        "$type": "plans/question/v1",
        "question": format!("Approve {target}?"),
        "asked": "2026-11-24T10:00:00Z",
        "by": {"session": "local_1"},
        "status": "open",
        "options": [{"id": "approve", "text": "Approve"}, {"id": "refuse", "text": "Refuse"}],
    });
    let answer = json!({
        "$type": "plans/answer/v1",
        "question": "/projects/p/questions/q",
        "choice": "approve",
        "words": "yes",
        "answered": "2026-11-24T10:05:00Z",
        "by": by,
    });
    mine.write(
        "projects/p/questions/q.json",
        Item::File(&question.to_string()),
    )
    .write("projects/p/answers/q.json", Item::File(&answer.to_string()));
}

/// `scenario` opened by `owner`, whose puts are checked as `board put`
/// checks them.
fn owning(scenario: &Scenario, owner: Asker) -> Board {
    let mut board = Board::open(scenario.workdir(), None).unwrap();
    board.ask_as(Asker {
        sign_owner_writes: true,
        ..owner
    });
    board
}

fn seen_at(mine: &Scenario, id: &str) -> Seen {
    Seen::from_value(&read(mine, &format!("effects/seen/{id}.json"))).unwrap()
}

#[test]
fn with_signing_off_an_approval_needs_an_answer_and_one_that_names_the_owner_approves() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    let owner = off(&mine);
    let bare = answer(&owner, &id, Answer::Approve, None, None, None).unwrap_err();
    assert_eq!(bare.code(), Code::SigningNotApproved, "{bare}");
    assert!(!exists(&mine, &format!("effects/seen/{id}.json")));
    answered(&mine, &id, &json!({"session": "local_1"}));
    let unnamed = answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    );
    let unnamed = unnamed.unwrap_err();
    assert_eq!(unnamed.code(), Code::SigningNotApproved);
    assert!(unnamed.to_string().contains("by.person"), "{unnamed}");
    answered(&mine, &id, &json!({"person": "ana", "session": "local_1"}));
    answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap();
    let seen = seen_at(&mine, &id);
    assert_eq!(
        (seen.approved.as_deref(), seen.signature),
        (Some(ANSWER), None)
    );
    let mut deciding = Board::open(scenario.workdir(), None).unwrap();
    deciding.ask_as(Asker {
        plugin: Some("review".parse().unwrap()),
        ..off(&mine)
    });
    assert_eq!(decide(&deciding, "/pulls/7").unwrap().effect, Some(id));
}

#[test]
fn with_signing_on_and_effects_listed_an_answer_does_not_approve() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let owner = asker(None, Some(&mine));
    let refused = answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    );
    assert_eq!(refused.unwrap_err().code(), Code::SigningNotApproved);
    let mut by_hand = Seen::new(&id, Answer::Approve, None);
    by_hand.approved = Some(ANSWER.to_owned());
    mine.write(
        &format!("effects/seen/{id}.json"),
        Item::File(&json!(by_hand).to_string()),
    );
    let error = decide(&board, "/pulls/7").unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
}

#[test]
fn with_signing_on_and_effects_unlisted_an_answer_approves() {
    let (scenario, mine) = (board(), personal());
    set(&mine, &json!({"require": ["order"]}));
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let owner = asker(None, Some(&mine));
    answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap();
    assert_eq!(decide(&board, "/pulls/7").unwrap().effect, Some(id));
}

#[test]
fn the_first_verified_signature_marks_signing_configured_in_the_answers_commit() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let first = asked(decide(&board, "/pulls/7"));
    let refused = asked(decide(&board, "/pulls/8"));
    let owner = asker(None, Some(&mine));
    answer(&owner, &refused, Answer::Refuse, None, None, None).unwrap();
    assert_eq!(read(&mine, ".board/board.json").get("signing"), None);
    answer(&owner, &first, Answer::Approve, None, None, None).unwrap();
    let config = read(&mine, ".board/board.json");
    assert_eq!(config["signing"], json!({"configured": true}));
    assert_eq!(config["commit_on_put"], json!(false));
}

#[test]
fn once_configured_a_missing_signers_file_refuses_each_listed_act_and_takes_no_answer() {
    let (scenario, mine) = (board(), personal());
    set(&mine, &json!({"configured": true}));
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let owner = missing(&mine);
    for approved in [None, Some(ANSWER.to_owned())] {
        let error = answer(&owner, &id, Answer::Approve, None, None, approved).unwrap_err();
        assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
        assert!(matches!(error, EffectError::FileMissing { .. }));
    }
    answer(&owner, &id, Answer::Refuse, None, None, None).unwrap();
    let mut reading = Board::open(scenario.workdir(), None).unwrap();
    reading.ask_as(Asker {
        plugin: Some("review".parse().unwrap()),
        ..missing(&mine)
    });
    let other = asked(decide(&board, "/pulls/8"));
    let mut by_hand = Seen::new(&other, Answer::Approve, None);
    by_hand.approved = Some(ANSWER.to_owned());
    mine.write(
        &format!("effects/seen/{other}.json"),
        Item::File(&json!(by_hand).to_string()),
    );
    assert_eq!(code(decide(&reading, "/pulls/8")), Code::SigningFileMissing);
    let writing = owning(&mine, missing(&mine));
    let error = put(&writing, "/effects/seen/s", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    assert!(!exists(&mine, "effects/seen/s.json"));
    put(&writing, "/tasks/a", &task("")).unwrap();
}

fn code<T: std::fmt::Debug>(result: Result<T, EffectError>) -> Code {
    result.unwrap_err().code()
}

#[test]
fn with_signing_off_an_owners_put_under_effects_needs_no_signature() {
    let mine = personal();
    let writing = owning(&mine, off(&mine));
    put(&writing, "/effects/seen/s", &task("")).unwrap();
    assert!(exists(&mine, "effects/seen/s.json"));
}

#[test]
fn with_signing_on_and_owners_writes_unlisted_a_put_under_effects_needs_no_signature() {
    let mine = personal();
    set(&mine, &json!({"require": ["effect"]}));
    let no_signer = Asker {
        signing: Signing {
            signer: None,
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    put(&owning(&mine, no_signer), "/effects/seen/s", &task("")).unwrap();
    assert!(exists(&mine, "effects/seen/s.json"));
}

/// The config `mine` holds with `signing` set to `setting`, as `board put`
/// would be given it.
fn config_with(setting: &Value) -> String {
    json!({"$type": "board/config/v1", "commit_on_put": false, "signing": setting}).to_string()
}

#[test]
fn an_edit_of_signing_needs_touch_id_while_signing_is_on_even_when_owners_writes_are_unlisted() {
    let mine = personal();
    set(&mine, &json!({"require": []}));
    let no_signer = Asker {
        signing: Signing {
            signer: None,
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    let unsigned = owning(&mine, no_signer.clone());
    let error = put(&unsigned, "/.board/board", &config_with(&json!({}))).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
    assert_eq!(
        read(&mine, ".board/board.json")["signing"],
        json!({"require": []})
    );
    let unchanged = json!({
        "$type": "board/config/v1",
        "commit_on_put": false,
        "plugins": [],
        "signing": {"require": []},
    });
    put(&unsigned, "/.board/board", &unchanged.to_string()).unwrap();
    let signed = owning(&mine, asker(None, Some(&mine)));
    put(
        &signed,
        "/.board/board",
        &config_with(&json!({"require": ["order"]})),
    )
    .unwrap();
    assert_eq!(
        read(&mine, ".board/board.json")["signing"],
        json!({"require": ["order"]})
    );
}

#[test]
fn with_signing_off_an_edit_of_signing_needs_no_signature() {
    let mine = personal();
    let writing = owning(&mine, off(&mine));
    put(
        &writing,
        "/.board/board",
        &config_with(&json!({"require": ["plan"]})),
    )
    .unwrap();
    assert_eq!(
        read(&mine, ".board/board.json")["signing"],
        json!({"require": ["plan"]})
    );
}

#[test]
fn once_configured_an_edit_of_signing_with_the_signers_file_missing_is_refused() {
    let mine = personal();
    set(&mine, &json!({"configured": true}));
    let writing = owning(&mine, missing(&mine));
    let error = put(&writing, "/.board/board", &config_with(&json!({}))).unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    assert_eq!(
        read(&mine, ".board/board.json")["signing"],
        json!({"configured": true})
    );
}
