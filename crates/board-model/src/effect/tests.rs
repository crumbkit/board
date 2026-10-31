#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests build each statement to check its layout"
)]
use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;

use super::*;

#[test]
fn an_effect_id_is_the_same_for_the_same_request_and_differs_for_another_body_path_or_prior_blob() {
    let effect = |path: &str, body: &[u8], prior: Option<&str>| Effect {
        digest: Some(Digest::of(body)),
        prior: prior.map(str::to_owned),
        ..Effect::new(
            "/b".to_owned(),
            "tasks".parse().unwrap(),
            "put".to_owned(),
            path.to_owned(),
        )
    };
    let id = effect("/x", b"x", None).id();
    assert!(is_effect_id(&id));
    assert_eq!(id, effect("/x", b"x", None).id());
    assert_ne!(id, effect("/y", b"x", None).id());
    assert_ne!(id, effect("/x", b"y", None).id());
    assert_ne!(id, effect("/x", b"x", Some("ab")).id());
    assert!(!is_effect_id(&id.to_uppercase()));
    assert!(!is_effect_id(&id[1..]));
}

#[test]
fn an_effect_and_a_seen_record_read_back_as_written_and_refuse_an_extra_field() {
    let mut effect = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "put".to_owned(),
        "/x".to_owned(),
    );
    effect.digest = Some(Digest::of(b"{}"));
    effect.record = Some(json!({"$type": "lr/task/v1"}));
    let value = serde_json::to_value(&effect).unwrap();
    assert_eq!(Effect::from_value(&value).unwrap(), effect);
    let mut extra = value;
    extra["extra"] = json!(1);
    assert!(effect_record(EFFECT_TYPE, &extra).unwrap().is_err());
    let mut seen = Seen::new(
        format!("/effects/pending/{}", effect.id()),
        Answer::Approve,
        None,
    );
    seen.signature = Some("-----BEGIN SSH SIGNATURE-----".to_owned());
    let value = serde_json::to_value(&seen).unwrap();
    assert_eq!(Seen::from_value(&value).unwrap(), seen);
    let mut extra = value;
    extra["extra"] = json!(1);
    assert!(effect_record(SEEN_TYPE, &extra).unwrap().is_err());
    assert!(effect_record("lr/task/v1", &extra).is_none());
}

#[test]
fn an_answer_reads_from_its_word() {
    assert_eq!("approve".parse(), Ok(Answer::Approve));
    assert_eq!("refuse".parse(), Ok(Answer::Refuse));
    assert!("yes".parse::<Answer>().is_err());
}

#[test]
fn an_approval_names_the_effect_by_its_id_and_shows_hidden_characters_escaped() {
    let effect = Effect::new(
        "/Users/o/b\u{202e}d".to_owned(),
        "review".parse().unwrap(),
        "review.comment".to_owned(),
        "/pulls/7".to_owned(),
    );
    assert_eq!(
        effect.approval(),
        format!(
            "board/approve/v1\neffect: {}\nplugin: review\nop: review.comment\npath: /pulls/7\nboard: /Users/o/b\\u{{202e}}d\n",
            effect.id()
        )
    );
    let mut other = effect.clone();
    other.path = "/pulls/8".to_owned();
    assert_ne!(other.approval(), effect.approval());
}

/// Every stored signature is checked against this text, so a change to it
/// needs a new first line.
#[test]
fn an_approval_of_a_put_keeps_its_frozen_layout_byte_for_byte() {
    let mut put = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "put".to_owned(),
        "/t/a".to_owned(),
    );
    put.record = Some(json!({"title": "a\u{200b}b", "$type": "lr/task/v1"}));
    assert_eq!(
        put.approval(),
        format!(
            "board/approve/v1\neffect: {}\nplugin: tasks\nop: put\npath: /t/a\nboard: /b\ncontent: 47 bytes in 4 lines\n{{\n  \"$type\": \"lr/task/v1\",\n  \"title\": \"a\\u{{200b}}b\"\n}}\n",
            put.id()
        )
    );
}

#[test]
fn an_approval_shows_text_split_at_each_newline_so_a_carriage_return_and_a_final_newline_show() {
    let mut file = Effect::new(
        "/b".to_owned(),
        "notes".parse().unwrap(),
        "put".to_owned(),
        "/n/a".to_owned(),
    );
    file.text = Some("one\r\ntwo\u{202e}\n".to_owned());
    assert!(
        file.approval()
            .ends_with("content: 12 bytes in 3 lines\none\\r\ntwo\\u{202e}\n\n"),
        "{}",
        file.approval()
    );
    let mut plain = file.clone();
    plain.text = Some("one\ntwo\u{202e}".to_owned());
    assert!(
        plain
            .approval()
            .ends_with("content: 10 bytes in 2 lines\none\ntwo\\u{202e}\n"),
        "{}",
        plain.approval()
    );
}

#[test]
fn an_owners_write_names_its_path_and_board_and_shows_its_content_and_never_reads_as_an_approval() {
    let record = json!({"answer": "refuse"});
    let write = Effect::owner_write("/b", "put", "/effects/seen/r\u{202e}", Some(&record), None);
    assert_eq!(
        write,
        "board/write/v1\nop: put\npath: /effects/seen/r\\u{202e}\nboard: /b\ncontent: 24 bytes in 3 lines\n{\n  \"answer\": \"refuse\"\n}\n"
    );
    let mut put = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "put".to_owned(),
        "/effects/seen/r\u{202e}".to_owned(),
    );
    put.record = Some(record);
    assert_ne!(write, put.approval());
    assert!(!write.starts_with(APPROVAL_HEADER));
    assert_eq!(signature_namespace(&write), "board/write/v1");
    assert_eq!(signature_namespace(&put.approval()), "board/approve/v1");
}

#[test]
fn an_effect_reads_only_when_its_plugin_is_a_plugins_name() {
    let effect = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "tasks.close".to_owned(),
        "/t/a".to_owned(),
    );
    let mut value = serde_json::to_value(&effect).unwrap();
    assert_eq!(Effect::from_value(&value).unwrap(), effect);
    value["plugin"] = json!("Tasks");
    assert_eq!(
        effect_record(EFFECT_TYPE, &value).unwrap(),
        Err(r#""Tasks" is not a name: use lowercase ASCII letters, digits and hyphens"#.to_owned())
    );
}

#[test]
fn only_a_statement_whose_whole_first_line_is_the_write_header_is_signed_as_a_write() {
    assert_eq!(
        signature_namespace("board/write/v1\nop: put\n"),
        "board/write/v1"
    );
    assert_eq!(
        signature_namespace("board/write/v1x\nop: put\n"),
        "board/approve/v1"
    );
    assert_eq!(signature_namespace("board/write/v1"), "board/approve/v1");
    assert_eq!(
        signature_namespace("op: put\nboard/write/v1\n"),
        "board/approve/v1"
    );
}

/// A decide that shows text: its id covers the digest, and its approval
/// shows the bytes exactly, the final newline as an empty last line.
#[test]
fn a_decide_showing_text_pins_its_id_and_approval_and_the_same_decide_with_other_text_differs() {
    let shown = |text: &str| {
        Effect::new(
            "/Users/o/board".to_owned(),
            "review".parse().unwrap(),
            "review.post".to_owned(),
            "/pulls/7".to_owned(),
        )
        .showing(None, Some(text.to_owned()))
    };
    let effect = shown("Looks good.\n");
    assert_eq!(effect.digest, Some(Digest::of(b"Looks good.\n")));
    // SHA-256 of the canonical JSON of the request, digest included.
    let id = "0bbec96799c1d0ea001469c436608a49ddede4809cfcbfe781553ab505f04571";
    assert_eq!(effect.id(), id);
    assert_eq!(
        effect.approval(),
        format!(
            "board/approve/v1\neffect: {id}\nplugin: review\nop: review.post\npath: /pulls/7\nboard: /Users/o/board\ncontent: 12 bytes in 2 lines\nLooks good.\n\n"
        )
    );
    assert_ne!(shown("Looks good.").id(), id);
}

#[test]
fn shown_text_and_a_shown_record_take_the_digest_of_their_bytes_and_nothing_shown_takes_none() {
    let record = json!({"b": 1, "a": 2});
    assert_eq!(
        shown_digest(Some(&record), None),
        Some(Digest::of(b"{\n  \"a\": 2,\n  \"b\": 1\n}\n"))
    );
    // A record's canonical JSON ends in a newline, which text may not.
    assert_eq!(
        shown_digest(None, Some("{}\n")),
        shown_digest(Some(&json!({})), None)
    );
    assert_ne!(
        shown_digest(None, Some("{}")),
        shown_digest(Some(&json!({})), None)
    );
    assert_eq!(shown_digest(None, None), None);
    assert_eq!(shown_digest(Some(&record), Some("x")), None);
}

#[test]
fn an_effect_asked_at_another_time_keeps_its_id_and_approval_and_reads_back_with_its_time() {
    let effect = Effect::new(
        "/b".to_owned(),
        "tasks".parse().unwrap(),
        "put".to_owned(),
        "/x".to_owned(),
    )
    .showing(None, Some("t\n".to_owned()));
    let asked = Effect {
        asked: Some(utc(UNIX_EPOCH + Duration::from_secs(1))),
        ..effect.clone()
    };
    assert_eq!(asked.id(), effect.id());
    assert_eq!(asked.approval(), effect.approval());
    let value = serde_json::to_value(&asked).unwrap();
    assert_eq!(value["asked"], "1970-01-01T00:00:01.000000Z");
    assert_eq!(Effect::from_value(&value).unwrap(), asked);
    assert!(
        serde_json::to_value(&effect)
            .unwrap()
            .get("asked")
            .is_none()
    );
}

#[test]
fn a_utc_time_is_rfc_3339_to_the_microsecond_with_a_fixed_width_of_27() {
    let at = |micros| utc(UNIX_EPOCH + Duration::from_micros(micros));
    assert_eq!(at(0), "1970-01-01T00:00:00.000000Z");
    let leap = at(1_709_251_199_000_000);
    assert_eq!(leap, "2024-02-29T23:59:59.000000Z");
    let now = at(1_793_400_294_102_864);
    assert_eq!(now, "2026-10-30T22:44:54.102864Z");
    assert_eq!((leap.len(), now.len()), (27, 27));
    assert!(leap < now);
}

#[test]
fn the_decide_limit_is_one_mebibyte() {
    assert_eq!(MAX_DECIDE_BYTES, 1_048_576);
}
