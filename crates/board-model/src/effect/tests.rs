use proptest::prelude::*;
use serde_json::json;

use super::*;

fn pattern(text: &str) -> Pattern {
    text.parse().unwrap()
}

/// A plain recursive matcher, slow and obviously right, to compare against.
fn reference(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| reference(rest, &text[skip..])),
        Some((&c, rest)) => text
            .split_first()
            .is_some_and(|(&t, more)| (c == '?' || c == t) && reference(rest, more)),
    }
}

fn rule(layer: Layer, op: &str, path: &str, authority: Authority, plugin: Option<&str>) -> Rule {
    Rule {
        layer,
        at: format!("/rules/{path}"),
        preference: Preference {
            plugin: plugin.map(str::to_owned),
            ..Preference::new(op.to_owned(), pattern(path), authority)
        },
    }
}

/// `op` at `path` in the board `/b`, asked by `plugin`.
fn request<'a>(op: &'a str, path: &'a str, plugin: Option<&'a str>) -> Request<'a> {
    Request {
        op,
        path,
        plugin,
        board: "/b",
    }
}

#[test]
fn a_pattern_must_start_with_a_slash_and_use_only_star_and_question_mark() {
    assert!("/tasks/*".parse::<Pattern>().is_ok());
    assert!("tasks/*".parse::<Pattern>().is_err());
    assert!("/tasks/?1".parse::<Pattern>().is_ok());
    for refused in ["/tasks/[ab]", "/tasks/{a,b}", "/tasks/\\*", "/tasks/a]"] {
        assert!(refused.parse::<Pattern>().is_err(), "{refused}");
    }
    for (refused, admitted) in [
        ("/tasks//a", "/tasks/a"),
        ("/tasks/./a", "/tasks/.a"),
        ("/tasks/../a", "/tasks/..a"),
        ("/tasks/", "/tasks"),
    ] {
        assert!(refused.parse::<Pattern>().is_err(), "{refused}");
        assert!(admitted.parse::<Pattern>().is_ok(), "{admitted}");
    }
    assert!("/".parse::<Pattern>().is_ok());
}

#[test]
fn a_star_crosses_slashes_and_a_question_mark_takes_one_character() {
    assert!(pattern("/tasks/*").matches("/tasks/a/b"));
    assert!(pattern("/*").matches("/"));
    assert!(!pattern("/tasks/*").matches("/notes/a"));
    assert!(pattern("/t?").matches("/t1"));
    assert!(!pattern("/t?").matches("/t"));
    assert!(!pattern("/t?").matches("/t12"));
    assert!(pattern("/a*b*c").matches("/a/xbyc"));
    assert!(!pattern("/a*b*c").matches("/a/xbyd"));
    assert!(pattern("/tasks/t1").is_exact());
    assert!(!pattern("/tasks/t?").is_exact());
}

#[test]
fn a_pattern_matches_ascii_letters_in_either_case_and_other_letters_only_as_written() {
    assert!(pattern("/Preferences/*").matches("/preferences/x"));
    assert!(pattern("/tasks/a").matches("/TASKS/A"));
    assert!(!pattern("/tasks/\u{e9}").matches("/tasks/\u{c9}"));
    assert!(pattern("/tasks/\u{e9}").matches("/TASKS/\u{e9}"));
}

/// A path of up to three short segments, or `/`.
fn text() -> impl Strategy<Value = String> {
    prop::collection::vec("[ab]{1,3}", 0..4).prop_map(|segments| format!("/{}", segments.join("/")))
}

/// A pattern that parses: no empty segment.
fn glob() -> impl Strategy<Value = String> {
    "/[ab/*?]{0,8}".prop_filter("a pattern that parses", |text| {
        text.parse::<Pattern>().is_ok()
    })
}

proptest! {
    #[test]
    fn the_matcher_agrees_with_a_plain_recursive_one(pattern in glob(), path in text()) {
        let chars: Vec<char> = pattern.chars().collect();
        let path_chars: Vec<char> = path.chars().collect();
        let parsed: Pattern = pattern.parse().unwrap();
        prop_assert_eq!(parsed.matches(&path), reference(&chars, &path_chars));
    }

    #[test]
    fn a_pattern_without_wildcards_matches_only_itself(path in text(), other in text()) {
        let parsed: Pattern = path.parse().unwrap();
        prop_assert!(parsed.matches(&path));
        prop_assert_eq!(parsed.matches(&other), other == path);
    }

    #[test]
    fn a_prefix_and_a_star_match_exactly_the_paths_under_the_prefix(prefix in text(), path in text()) {
        let parsed: Pattern = format!("{prefix}*").parse().unwrap();
        prop_assert_eq!(parsed.matches(&path), path.starts_with(&prefix));
    }

    #[test]
    fn a_question_mark_for_any_character_still_matches(path in "/[ab/]{1,8}", at in 1usize..9) {
        let mut chars: Vec<char> = path.chars().collect();
        let at = at % chars.len();
        prop_assume!(at > 0);
        chars[at] = '?';
        let parsed = chars.iter().collect::<String>().parse::<Pattern>();
        prop_assume!(parsed.is_ok());
        let parsed = parsed.unwrap();
        prop_assert!(parsed.matches(&path));
    }
}

#[test]
fn with_no_rule_a_put_acts_and_any_other_op_asks() {
    let none = decide(&[], &request(PUT_OP, "/tasks/t1", Some("tasks")));
    assert_eq!((none.authority, none.deciding), (Authority::Act, None));
    let other = decide(&[], &request("review.comment", "/tasks/t1", Some("tasks")));
    assert_eq!(other.authority, Authority::Ask);
}

#[test]
fn the_narrowest_matching_rule_decides_and_every_matching_rule_is_reported() {
    let rules = [
        rule(Layer::Board, "put", "/tasks/*", Authority::Act, None),
        rule(Layer::Owner, "put", "/tasks/t?", Authority::Ask, None),
        rule(Layer::Owner, "put", "/notes/*", Authority::Refuse, None),
        rule(Layer::Board, "put", "/*", Authority::Propose, None),
    ];
    let decision = decide(&rules, &request(PUT_OP, "/tasks/t1", Some("tasks")));
    assert_eq!(decision.authority, Authority::Ask);
    assert_eq!(decision.matching.len(), 3);
    assert_eq!(decision.matching[decision.deciding.unwrap()], rules[1]);
    // An owner's act never widens a board's narrower rule.
    let widening = [
        rule(Layer::Board, "put", "/tasks/*", Authority::Refuse, None),
        rule(Layer::Owner, "put", "/tasks/*", Authority::Act, None),
    ];
    let decision = decide(&widening, &request(PUT_OP, "/tasks/t1", None));
    assert_eq!(decision.authority, Authority::Refuse);
}

#[test]
fn a_rule_naming_a_plugin_covers_only_that_plugin_and_another_op_is_not_covered() {
    let rules = [rule(
        Layer::Owner,
        "put",
        "/*",
        Authority::Refuse,
        Some("tasks"),
    )];
    assert_eq!(
        decide(&rules, &request(PUT_OP, "/x", Some("tasks"))).authority,
        Authority::Refuse
    );
    assert_eq!(
        decide(&rules, &request(PUT_OP, "/x", Some("notes"))).authority,
        Authority::Act
    );
    assert_eq!(
        decide(&rules, &request(PUT_OP, "/x", None)).authority,
        Authority::Act
    );
    assert_eq!(
        decide(&rules, &request("tasks.due", "/x", Some("tasks"))).authority,
        Authority::Ask
    );
}

#[test]
fn a_rule_naming_a_board_covers_only_requests_in_that_board() {
    let mut named = rule(Layer::Owner, "put", "/*", Authority::Refuse, None);
    named.preference.board = Some("/b".to_owned());
    let rules = [named];
    assert_eq!(
        decide(&rules, &request(PUT_OP, "/x", Some("tasks"))).authority,
        Authority::Refuse
    );
    let elsewhere = Request {
        board: "/c",
        ..request(PUT_OP, "/x", Some("tasks"))
    };
    assert_eq!(decide(&rules, &elsewhere).authority, Authority::Act);
}

#[test]
fn tell_comes_from_the_deciding_rule() {
    let mut told = rule(Layer::Owner, "put", "/*", Authority::Ask, None);
    told.preference.tell = Some(Tell::Now);
    let decision = decide(&[told], &request(PUT_OP, "/x", None));
    assert_eq!(decision.tell(), Some(Tell::Now));
    assert_eq!(decide(&[], &request(PUT_OP, "/x", None)).tell(), None);
}

#[test]
fn an_effect_id_is_the_same_for_the_same_request_and_differs_for_another_body_path_or_prior_blob() {
    let effect = |path: &str, body: &[u8], prior: Option<&str>| Effect {
        digest: Some(Digest::of(body)),
        prior: prior.map(str::to_owned),
        ..Effect::new(
            "/b".to_owned(),
            "tasks".to_owned(),
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
fn a_preference_reads_with_its_fields_and_refuses_an_extra_one_or_a_bad_op_or_pattern() {
    let value = json!({
        "$type": PREFERENCE_TYPE,
        "op": "put",
        "path": "/tasks/*",
        "authority": "ask",
        "plugin": "tasks",
        "board": "/b",
        "tell": "summary",
    });
    let preference = Preference::from_value(&value).unwrap();
    assert_eq!(preference.tell, Some(Tell::Summary));
    assert_eq!(serde_json::to_value(&preference).unwrap(), value);
    for (field, refused) in [
        ("extra", json!(1)),
        ("op", json!("comment")),
        ("path", json!("/tasks/[a]")),
        ("authority", json!("maybe")),
    ] {
        let mut changed = value.clone();
        changed[field] = refused;
        assert!(Preference::from_value(&changed).is_err(), "{field}");
    }
    assert!(effect_record(PREFERENCE_TYPE, &value).unwrap().is_ok());
    assert!(effect_record("lr/task/v1", &value).is_none());
}

#[test]
fn an_effect_and_a_seen_record_read_back_as_written_and_refuse_an_extra_field() {
    let mut effect = Effect::new(
        "/b".to_owned(),
        "tasks".to_owned(),
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
    let seen = Seen::new(
        format!("/effects/pending/{}", effect.id()),
        Answer::Approve,
        None,
    );
    let value = serde_json::to_value(&seen).unwrap();
    assert_eq!(Seen::from_value(&value).unwrap(), seen);
    let mut extra = value;
    extra["extra"] = json!(1);
    assert!(effect_record(SEEN_TYPE, &extra).unwrap().is_err());
}

#[test]
fn an_answer_reads_from_its_word_and_gives_the_authority_a_saved_rule_has() {
    assert_eq!("approve".parse(), Ok(Answer::Approve));
    assert!("yes".parse::<Answer>().is_err());
    assert!("propose".parse::<Answer>().is_err());
    assert_eq!(Answer::Approve.authority(), Authority::Act);
    assert_eq!(Answer::Refuse.authority(), Authority::Refuse);
    assert!(Authority::Act < Authority::Propose);
    assert!(Authority::Ask < Authority::Refuse);
}
