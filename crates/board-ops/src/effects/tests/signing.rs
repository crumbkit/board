//! Optional signing: each act with signing off, on and listed, on and
//! unlisted, and configured with the allowed signers file missing.

use std::path::PathBuf;

use board_model::{Answer, Code, Seen};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::{
    Asker, EffectError, Signing, answer, asked, asker, board, decide, exists, now, opened,
    personal, put, read, rm, signer, signing,
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
/// choosing `approve`, by `by`, at [`ANSWER`], answered a minute ago.
fn answered(mine: &Scenario, target: &str, by: &Value) {
    answered_at(mine, target, by, -60);
}

/// [`answered`], answered `offset` seconds from now and asked a minute
/// before that.
fn answered_at(mine: &Scenario, target: &str, by: &Value, offset: i64) {
    let question = json!({
        "$type": "plans/question/v1",
        "question": format!("Approve {target}?"),
        "asked": now(offset - 60),
        "by": {"session": "local_1"},
        "status": "open",
        "options": [{"id": "approve", "text": "Approve"}, {"id": "refuse", "text": "Refuse"}],
    });
    let answer = json!({
        "$type": "plans/answer/v1",
        "question": "/projects/p/questions/q",
        "choice": "approve",
        "words": "yes",
        "answered": now(offset),
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
fn a_signed_approval_in_the_board_makes_signing_configured_without_writing_the_config() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let first = asked(decide(&board, "/pulls/7"));
    let second = asked(decide(&board, "/pulls/8"));
    let owner = asker(None, Some(&mine));
    let before = read(&mine, ".board/board.json");
    answer(&owner, &first, Answer::Approve, None, None, None).unwrap();
    assert_eq!(read(&mine, ".board/board.json"), before);
    answered(&mine, &second, &json!({"person": "ana"}));
    let error = answer(
        &missing(&mine),
        &second,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
}

#[test]
fn a_board_holding_a_signed_order_refuses_when_the_signers_file_is_gone_before_any_flag_is_set() {
    let (scenario, mine) = (board(), personal());
    // Only that a signature is there counts, so any text stands in for one.
    let order = json!({"$type": board_model::ORDER_TYPE, "signature": "signed"});
    mine.write("orders/deadband.json", Item::File(&order.to_string()));
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let error = answer(
        &missing(&mine),
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    set(&mine, &json!({"configured": false}));
    answer(
        &missing(&mine),
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap();
}

#[test]
fn a_dangling_signers_link_refuses_each_listed_act_with_no_signature_ever_verified() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let link = signer().directory().join("dangling");
    std::os::unix::fs::symlink(signer().directory().join("nowhere"), &link).unwrap();
    let owner = Asker {
        signing: Signing {
            allowed: Some(link),
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    let error = answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    );
    assert_eq!(code(error), Code::SigningFileMissing);
}

#[test]
fn an_owner_line_in_the_attention_signers_file_alone_turns_signing_on() {
    let (scenario, mine) = (board(), personal());
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let owner = Asker {
        signing: Signing {
            allowed: Some(no_owner()),
            attention_allowed: Some(signer().allowed()),
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    let error = answer(
        &owner,
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    );
    assert_eq!(code(error), Code::SigningNotApproved);
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
fn with_signing_off_an_edit_of_require_needs_an_approving_answer_that_names_the_new_setting() {
    let mine = personal();
    let require = json!({"require": ["plan"]});
    let bare = put(
        &owning(&mine, off(&mine)),
        "/.board/board",
        &config_with(&require),
    );
    assert_eq!(bare.unwrap_err().code(), Code::SigningNotApproved);
    assert_eq!(read(&mine, ".board/board.json")["signing"], Value::Null);
    answered(
        &mine,
        &board_model::setting_digest(Some(&json!({"require": []}))),
        &json!({"person": "ana"}),
    );
    let approving = Asker {
        approved: Some(ANSWER.to_owned()),
        ..off(&mine)
    };
    let other = put(
        &owning(&mine, approving.clone()),
        "/.board/board",
        &config_with(&require),
    );
    assert_eq!(other.unwrap_err().code(), Code::SigningNotApproved);
    answered(
        &mine,
        &board_model::setting_digest(Some(&require)),
        &json!({"person": "ana"}),
    );
    put(
        &owning(&mine, approving),
        "/.board/board",
        &config_with(&require),
    )
    .unwrap();
    assert_eq!(read(&mine, ".board/board.json")["signing"], require);
}

#[test]
fn with_signing_off_an_edit_of_configured_is_refused_even_with_an_approving_answer() {
    let mine = personal();
    let configured = json!({"configured": false});
    answered(
        &mine,
        &board_model::setting_digest(Some(&configured)),
        &json!({"person": "ana"}),
    );
    let approving = Asker {
        approved: Some(ANSWER.to_owned()),
        ..off(&mine)
    };
    let error = put(
        &owning(&mine, approving),
        "/.board/board",
        &config_with(&configured),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
    assert!(error.to_string().contains("signing is off"), "{error}");
    assert_eq!(read(&mine, ".board/board.json")["signing"], Value::Null);
}

#[test]
fn configured_false_written_while_off_cannot_keep_signing_off_once_the_owner_signs_and_loses_the_file()
 {
    let (scenario, mine) = (board(), personal());
    let configured = json!({"configured": false});
    let error = put(
        &owning(&mine, off(&mine)),
        "/.board/board",
        &config_with(&configured),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
    // The owner adds their line and signs approvals, then the file goes.
    let board = opened(&scenario, Some("review"), Some(&mine));
    let first = asked(decide(&board, "/pulls/7"));
    let second = asked(decide(&board, "/pulls/8"));
    answer(
        &asker(None, Some(&mine)),
        &first,
        Answer::Approve,
        None,
        None,
        None,
    )
    .unwrap();
    answered(&mine, &second, &json!({"person": "ana"}));
    let error = answer(
        &missing(&mine),
        &second,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
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

/// Writes in `mine` an order with a signature, which keeps signing
/// configured. Only that a signature is there counts.
fn signed_order(mine: &Scenario) {
    let order = json!({"$type": board_model::ORDER_TYPE, "signature": "signed"});
    mine.write("orders/deadband.json", Item::File(&order.to_string()));
}

#[test]
fn while_signing_is_on_removing_or_unsigning_a_signed_record_needs_touch_id_whatever_require_lists()
{
    let mine = personal();
    set(&mine, &json!({"require": []}));
    signed_order(&mine);
    let no_signer = Asker {
        signing: Signing {
            signer: None,
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    let unsigned = owning(&mine, no_signer);
    let error = rm(&unsigned, "/orders/deadband").unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
    let error = put(&unsigned, "/orders/deadband", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SettingsUnsigned, "{error}");
    assert!(exists(&mine, "orders/deadband.json"));
    let gone = owning(&mine, missing(&mine));
    let error = rm(&gone, "/orders/deadband").unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    rm(&owning(&mine, asker(None, Some(&mine))), "/orders/deadband").unwrap();
    assert!(!exists(&mine, "orders/deadband.json"));
}

#[test]
fn removing_the_signed_records_and_then_the_signers_file_does_not_turn_signing_off() {
    let (scenario, mine) = (board(), personal());
    signed_order(&mine);
    let board = opened(&scenario, Some("review"), Some(&mine));
    let id = asked(decide(&board, "/pulls/7"));
    answered(&mine, &id, &json!({"person": "ana"}));
    let no_signer = Asker {
        signing: Signing {
            signer: None,
            ..signing()
        },
        ..asker(None, Some(&mine))
    };
    assert!(rm(&owning(&mine, no_signer), "/orders/deadband").is_err());
    let mut plugin = Board::open(mine.workdir(), None).unwrap();
    plugin.ask_as(asker(Some("review"), Some(&mine)));
    let error = rm(&plugin, "/orders/deadband").unwrap_err();
    assert_eq!(error.code(), Code::SettingsFromPlugin, "{error}");
    let error = answer(
        &missing(&mine),
        &id,
        Answer::Approve,
        None,
        None,
        Some(ANSWER.to_owned()),
    );
    assert_eq!(code(error), Code::SigningFileMissing);
}

#[test]
fn with_signing_off_an_answer_from_over_30_days_ago_does_not_approve_an_edit_of_require() {
    let mine = personal();
    let require = json!({"require": ["plan"]});
    let digest = board_model::setting_digest(Some(&require));
    let approving = Asker {
        approved: Some(ANSWER.to_owned()),
        ..off(&mine)
    };
    let days = |days: i64| days * 24 * 60 * 60;
    answered_at(&mine, &digest, &json!({"person": "ana"}), -days(31));
    let stale = put(
        &owning(&mine, approving.clone()),
        "/.board/board",
        &config_with(&require),
    )
    .unwrap_err();
    assert_eq!(stale.code(), Code::SigningNotApproved, "{stale}");
    assert!(stale.to_string().contains("30 days"), "{stale}");
    assert_eq!(read(&mine, ".board/board.json")["signing"], Value::Null);
    answered_at(&mine, &digest, &json!({"person": "ana"}), -days(29));
    put(
        &owning(&mine, approving),
        "/.board/board",
        &config_with(&require),
    )
    .unwrap();
    assert_eq!(read(&mine, ".board/board.json")["signing"], require);
}

#[test]
fn with_no_personal_board_named_a_signed_record_in_the_written_board_keeps_a_missing_signers_file_refusing()
 {
    let mine = personal();
    signed_order(&mine);
    let require = json!({"require": ["plan"]});
    answered(
        &mine,
        &board_model::setting_digest(Some(&require)),
        &json!({"person": "ana"}),
    );
    let unnamed = Asker {
        personal: None,
        approved: Some(ANSWER.to_owned()),
        ..missing(&mine)
    };
    let writing = owning(&mine, unnamed.clone());
    let error = put(&writing, "/.board/board", &config_with(&require)).unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    assert_eq!(read(&mine, ".board/board.json")["signing"], Value::Null);
    let error = put(&writing, "/effects/seen/s", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::SigningFileMissing, "{error}");
    assert!(!exists(&mine, "effects/seen/s.json"));
    put(&writing, "/tasks/a", &task("")).unwrap();
    // With no signed record, the same write reads signing as off.
    let other = personal();
    answered(
        &other,
        &board_model::setting_digest(Some(&require)),
        &json!({"person": "ana"}),
    );
    put(
        &owning(
            &other,
            Asker {
                personal: None,
                ..unnamed
            },
        ),
        "/.board/board",
        &config_with(&require),
    )
    .unwrap();
    assert_eq!(read(&other, ".board/board.json")["signing"], require);
}

#[test]
fn a_personal_board_that_cannot_be_named_refuses_only_the_writes_signing_governs() {
    let mine = personal();
    let refusal = board_model::Refusal::new(
        Code::ConfigNotFound,
        "`personal` names a directory that is not the root of a board",
        &board_model::Fields::default(),
    );
    let unopened = Asker {
        personal: None,
        unopened: Some(Box::new(refusal)),
        ..off(&mine)
    };
    let writing = owning(&mine, unopened);
    put(&writing, "/tasks/a", &task("")).unwrap();
    let error = put(&writing, "/effects/seen/s", &task("")).unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound, "{error}");
    assert_eq!(
        error.refusal().message,
        "`personal` names a directory that is not the root of a board"
    );
    assert!(!exists(&mine, "effects/seen/s.json"));
    let error = put(
        &writing,
        "/.board/board",
        &config_with(&json!({"require": []})),
    )
    .unwrap_err();
    assert_eq!(error.code(), Code::ConfigNotFound, "{error}");
}
