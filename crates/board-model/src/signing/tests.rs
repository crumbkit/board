use serde_json::json;

use super::{Act, Edit, Need, Signers, Signing, SigningSetting, SigningState};

const FILE: &str = "/Users/ana/signers";

fn setting(require: Option<&[Act]>, configured: bool) -> SigningSetting {
    SigningSetting {
        require: require.map(<[Act]>::to_vec),
        configured: configured.then_some(true),
    }
}

/// Signing's state for a board that holds no signed record.
fn signing(signers: Signers, setting: SigningSetting) -> Signing {
    Signing::new(signers, setting, false)
}

#[test]
fn the_default_lists_orders_effects_and_owners_writes() {
    let default = SigningSetting::default();
    for act in [Act::Order, Act::Effect, Act::OwnerWrite] {
        assert!(default.requires(act), "{act:?}");
    }
    for act in [Act::Plan, Act::Data, Act::Mockup] {
        assert!(!default.requires(act), "{act:?}");
    }
}

#[test]
fn the_setting_reads_and_writes_its_json_and_refuses_an_unknown_act() {
    let read: SigningSetting =
        serde_json::from_value(json!({"require": ["order", "owner-write", "mockup"]})).unwrap();
    assert_eq!(
        read,
        setting(Some(&[Act::Order, Act::OwnerWrite, Act::Mockup]), false)
    );
    assert_eq!(
        serde_json::to_value(SigningSetting::default()).unwrap(),
        json!({})
    );
    let configured = serde_json::to_value(setting(None, true)).unwrap();
    assert_eq!(configured, json!({"configured": true}));
    assert!(serde_json::from_value::<SigningSetting>(json!({"require": ["push"]})).is_err());
    assert!(serde_json::from_value::<SigningSetting>(json!({"sign": []})).is_err());
}

#[test]
fn the_state_comes_from_the_owner_line_and_the_configured_flag() {
    let state = |signers, configured| signing(signers, setting(None, configured)).state;
    assert_eq!(state(Signers::Missing, false), SigningState::Off);
    assert_eq!(state(Signers::NoOwner, false), SigningState::Off);
    assert_eq!(state(Signers::Owner, false), SigningState::On);
    assert_eq!(state(Signers::Owner, true), SigningState::Configured);
    assert_eq!(state(Signers::NoOwner, true), SigningState::Configured);
    assert_eq!(state(Signers::Missing, true), SigningState::FileMissing);
}

#[test]
fn a_board_that_holds_a_signed_record_is_configured_unless_the_owner_set_configured_false() {
    let held = |signers, configured: Option<bool>| {
        let setting = SigningSetting {
            require: None,
            configured,
        };
        Signing::new(signers, setting, true).state
    };
    assert_eq!(held(Signers::Missing, None), SigningState::FileMissing);
    assert_eq!(held(Signers::NoOwner, None), SigningState::Configured);
    assert_eq!(held(Signers::Missing, Some(false)), SigningState::Off);
    let read: SigningSetting = serde_json::from_value(json!({"configured": false})).unwrap();
    assert_eq!(read.configured, Some(false));
}

#[test]
fn a_signers_file_board_cannot_read_refuses_the_listed_acts_whatever_configured_says() {
    for configured in [None, Some(false), Some(true)] {
        let setting = SigningSetting {
            require: None,
            configured,
        };
        let unreadable = Signing::new(Signers::Unreadable, setting, false);
        assert_eq!(
            unreadable.state,
            SigningState::FileMissing,
            "{configured:?}"
        );
        assert_eq!(unreadable.need(Act::Effect), Need::FileMissing);
        assert_eq!(unreadable.need(Act::Plan), Need::Answer);
    }
}

#[test]
fn with_signing_off_every_act_takes_an_answer() {
    for signers in [Signers::Missing, Signers::NoOwner] {
        let off = signing(signers, setting(Some(&Act::ALL), false));
        for act in Act::ALL {
            assert_eq!(off.need(act), Need::Answer, "{act:?}");
        }
        assert_eq!(off.need_to_edit(Edit::Require), Need::Answer);
    }
}

#[test]
fn an_edit_of_configured_needs_a_signature_even_with_signing_off_and_one_of_require_an_answer() {
    for signers in [Signers::Missing, Signers::NoOwner] {
        let off = signing(signers, SigningSetting::default());
        assert_eq!(off.need_to_edit(Edit::Configured), Need::Signature);
        assert_eq!(off.need_to_edit(Edit::Require), Need::Answer);
    }
    let on = signing(Signers::Owner, SigningSetting::default());
    assert_eq!(on.need_to_edit(Edit::Configured), Need::Signature);
    assert_eq!(on.need_to_edit(Edit::Require), Need::Signature);
}

#[test]
fn with_signing_on_a_listed_act_needs_a_signature_and_an_unlisted_one_an_answer() {
    for signers in [Signers::Owner, Signers::NoOwner] {
        let configured = signers == Signers::NoOwner;
        let on = signing(
            signers,
            setting(Some(&[Act::Effect, Act::Plan]), configured),
        );
        assert_eq!(on.need(Act::Effect), Need::Signature);
        assert_eq!(on.need(Act::Plan), Need::Signature);
        assert_eq!(on.need(Act::Order), Need::Answer);
        assert_eq!(on.need(Act::OwnerWrite), Need::Answer);
        assert_eq!(on.need(Act::Mockup), Need::Answer);
    }
}

#[test]
fn an_edit_of_the_setting_needs_a_signature_whenever_signing_is_on() {
    let unlisted = setting(Some(&[]), false);
    assert_eq!(
        signing(Signers::Owner, unlisted.clone()).need_to_edit(Edit::Require),
        Need::Signature
    );
    let configured = setting(Some(&[]), true);
    assert_eq!(
        signing(Signers::NoOwner, configured.clone()).need_to_edit(Edit::Require),
        Need::Signature
    );
    assert_eq!(
        signing(Signers::Missing, configured).need_to_edit(Edit::Configured),
        Need::FileMissing
    );
}

#[test]
fn once_configured_a_missing_file_refuses_listed_acts_and_never_falls_back() {
    let missing = signing(Signers::Missing, setting(None, true));
    for act in [Act::Order, Act::Effect, Act::OwnerWrite] {
        assert_eq!(missing.need(act), Need::FileMissing, "{act:?}");
    }
    assert_eq!(missing.need(Act::Plan), Need::Answer);
}

#[test]
fn the_state_line_names_the_state_and_the_listed_acts() {
    let line = |signers, require: Option<&[Act]>, configured| {
        signing(signers, setting(require, configured)).line(FILE)
    };
    assert_eq!(
        line(Signers::Missing, None, false),
        "Signing is off: every act takes an answer in chat."
    );
    assert_eq!(
        line(Signers::Owner, None, false),
        "Signing is on: orders, effects and owner's writes need Touch ID."
    );
    assert_eq!(
        line(Signers::Owner, Some(&[Act::Order]), true),
        "Signing is on: orders need Touch ID."
    );
    assert_eq!(
        line(Signers::Owner, Some(&[Act::Data, Act::Mockup]), false),
        "Signing is on: data sign-offs and mockups need Touch ID."
    );
    assert_eq!(
        line(Signers::Owner, Some(&[]), false),
        "Signing is on: no act needs Touch ID."
    );
    assert_eq!(
        line(Signers::Missing, None, true),
        "Signing is on, but /Users/ana/signers is missing: orders, effects and owner's writes are refused until it is restored."
    );
}

#[test]
fn the_signers_line_names_the_file_with_no_owner_line_when_signing_is_off() {
    let off = signing(Signers::NoOwner, SigningSetting::default());
    assert_eq!(
        off.signers_line(FILE),
        "Signing is off: /Users/ana/signers has no owner line."
    );
    let on = signing(Signers::Owner, SigningSetting::default());
    assert_eq!(on.signers_line(FILE), on.line(FILE));
}

const EFFECT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn question() -> serde_json::Value {
    json!({
        "$type": "plans/question/v1",
        "question": format!("Approve pending effect {EFFECT}?"),
        "asked": "2026-11-24T10:00:00Z",
        "options": [{"id": "approve", "text": "Approve it"}, {"id": "refuse", "text": "Refuse it"}],
    })
}

fn answer(extra: &serde_json::Value) -> serde_json::Value {
    let mut answer = json!({
        "$type": "plans/answer/v1",
        "question": "/projects/p/questions/q",
        "choice": "approve",
        "words": "yes",
        "answered": "2026-11-24T10:05:00Z",
        "by": {"person": "ana", "session": "local_1"},
    });
    for (key, value) in extra.as_object().unwrap() {
        answer[key] = value.clone();
    }
    answer
}

#[test]
fn an_answer_heard_not_heard_or_from_an_owner_surface_approves_while_the_heard_check_is_silent() {
    use std::time::{Duration, UNIX_EPOCH};
    let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
    let question = question();
    let words = "Approve the review effect";
    let typed = answer(&json!({"words": words}));
    let heard = super::approving(&typed, Some(&question), EFFECT).unwrap();
    let log = format!("9999000 {}\n", super::prompt_digest(words));
    assert!(heard.heard(Some(&log), now));
    assert!(!heard.heard(None, now));
    // The stored field is the writer's claim, so the check never reads it.
    let claimed = answer(&json!({"words": words, "heard": true}));
    let claimed = super::approving(&claimed, Some(&question), EFFECT).unwrap();
    assert!(!claimed.heard(Some(""), now));
    assert_eq!(claimed.session.as_deref(), Some("local_1"));
    let pane = answer(&json!({"written_by": "plans/pane-answer"}));
    let surface = super::approving(&pane, Some(&question), EFFECT).unwrap();
    assert_eq!(surface.surface.as_deref(), Some("plans/pane-answer"));
}

#[test]
fn an_answer_with_no_person_does_not_approve() {
    let question = question();
    let unnamed = answer(&json!({"by": {"session": "local_1"}, "written_by": "plans/pane-answer"}));
    let why = super::approving(&unnamed, Some(&question), EFFECT).unwrap_err();
    assert!(why.contains("by.person"), "{why}");
    let empty = answer(&json!({"by": {"person": ""}}));
    assert!(super::approving(&empty, Some(&question), EFFECT).is_err());
}

#[test]
fn an_answer_approves_only_its_own_earlier_question_and_only_with_the_approve_option() {
    let question = question();
    let before = answer(&json!({"answered": "2026-11-24T09:59:59Z"}));
    let why = super::approving(&before, Some(&question), EFFECT).unwrap_err();
    assert!(why.contains("before"), "{why}");
    let refused = answer(&json!({"choice": "refuse"}));
    let why = super::approving(&refused, Some(&question), EFFECT).unwrap_err();
    assert!(why.contains("approve"), "{why}");
    let why = super::approving(&answer(&json!({})), Some(&question), "/orders/other").unwrap_err();
    assert!(why.contains("/orders/other"), "{why}");
    let why = super::approving(&answer(&json!({})), None, EFFECT).unwrap_err();
    assert!(why.contains("question"), "{why}");
    let decision = json!({"$type": "plans/decision/v1", "by": {"person": "ana"}});
    assert!(super::approving(&decision, Some(&question), EFFECT).is_err());
}

#[test]
fn words_are_heard_when_a_prompt_log_entry_from_the_last_30_days_holds_their_digest() {
    use std::time::{Duration, UNIX_EPOCH};
    let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
    let digest = super::prompt_digest("  Approve it, yes\n");
    assert_eq!(digest, super::prompt_digest("Approve it, yes"));
    assert_eq!(digest.len(), 64);
    let fresh = format!("9999000 {digest}\n");
    assert!(super::heard_in(&fresh, "Approve it, yes", now));
    assert!(!super::heard_in(&fresh, "Approve it", now));
    let old = format!("{} {digest}\n", 10_000_000 - 31 * 24 * 60 * 60);
    assert!(!super::heard_in(&old, "Approve it, yes", now));
    assert!(!super::heard_in("garbage\n\n", "Approve it, yes", now));
}

#[test]
fn one_word_or_a_digest_logged_after_now_is_never_heard() {
    use std::time::{Duration, UNIX_EPOCH};
    let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
    let yes = format!("9999000 {}\n", super::prompt_digest("yes"));
    assert!(!super::heard_in(&yes, "yes", now));
    assert!(!super::heard_in(&yes, "  yes \n", now));
    let words = "Approve it, yes";
    let later = format!("10000001 {}\n", super::prompt_digest(words));
    assert!(!super::heard_in(&later, words, now));
}

#[test]
fn the_prompt_digest_is_the_sha256_the_board_plans_mod_logs() {
    // The vector the board-plans mod's prompts test checks.
    assert_eq!(
        super::prompt_digest("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn a_setting_digest_names_the_current_and_the_new_setting_so_an_answer_approves_one_edit_from_one_setting()
 {
    let empty = json!({"require": []});
    let plan = json!({"require": ["plan"]});
    let short = super::setting_digest(None, Some(&empty));
    assert!(
        short.starts_with("sha256:") && short.len() == 7 + 64,
        "{short}"
    );
    assert_eq!(short, super::setting_digest(None, Some(&empty)));
    assert_ne!(short, super::setting_digest(None, Some(&plan)));
    assert_ne!(short, super::setting_digest(None, None));
    // The same new setting from another starting setting, or the edit back.
    assert_ne!(short, super::setting_digest(Some(&plan), Some(&empty)));
    assert_ne!(short, super::setting_digest(Some(&empty), None));
}

#[test]
fn an_answer_is_recent_when_answered_in_the_last_30_days_and_not_after_now() {
    use std::time::{Duration, UNIX_EPOCH};
    let answered = |seconds: u64| {
        let at = crate::utc(UNIX_EPOCH + Duration::from_secs(seconds));
        answer(&json!({"answered": at}))
    };
    let now = UNIX_EPOCH + Duration::from_secs(10_000_000);
    super::recent(&answered(9_999_000), now).unwrap();
    assert_eq!(super::APPROVAL_DAYS, 30);
    super::recent(&answered(10_000_000 - 30 * 24 * 60 * 60), now).unwrap();
    let why = super::recent(&answered(10_000_000 - 31 * 24 * 60 * 60), now).unwrap_err();
    assert!(why.contains("30 days"), "{why}");
    let why = super::recent(&answered(10_000_001), now).unwrap_err();
    assert!(why.contains("after"), "{why}");
    let why = super::recent(&answer(&json!({"answered": "soon"})), now).unwrap_err();
    assert!(why.contains("time"), "{why}");
}
