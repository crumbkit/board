//! A failed `boundary.check` asks the owner to override it, and the owner's
//! signed approval, given with `board answer`, lets that exact content pass.

use super::*;

/// A boundary that refuses private names in every part of a pull request
/// to octo.
fn covered(personal: &Personal) {
    private_names(personal);
    let fields = json!({"to": "github:octo/*", "actions": ["github.*"], "checks": names_check()});
    personal.write("boundaries/octo.json", &boundary(&fields));
}

/// A pull request to octo/board from no checkout, with `parts`.
fn request(parts: &Value) -> Value {
    json!({"action": "github.pr-create", "to": "github:octo/board", "parts": parts})
}

fn failing() -> Value {
    json!({"pr.title": "Fine", "pr.body": "See acme-123 here."})
}

/// What the owner is shown for `parts` sent to octo/board by
/// `github.pr-create`.
fn shown(parts: &[(&str, &str)]) -> String {
    let mut text = "action \"github.pr-create\"\nto \"github:octo/board\"\n".to_owned();
    for (name, body) in parts {
        text += &format!("\npart {name:?}\n");
        for line in body.split('\n') {
            text += &format!("  {line}\n");
        }
    }
    text
}

#[test]
fn a_failed_check_asks_the_owner_once_and_names_the_effect_at_the_end_of_its_hint() {
    let personal = Personal::new();
    covered(&personal);
    let (ran, document) = personal.call("check", &request(&failing()));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    let fields = &document["fields"];
    let effect = fields["effect"].as_str().unwrap();
    assert_eq!(personal.pending(), [effect]);
    let asked = (personal.record(&format!("/effects/pending/{effect}"))).unwrap();
    assert_eq!(
        (&asked["plugin"], &asked["op"], &asked["path"]),
        (
            &json!("boundary"),
            &json!("boundary.override"),
            &fields["crossing"]
        )
    );
    let expected = shown(&[("pr.body", "See acme-123 here."), ("pr.title", "Fine")]);
    assert_eq!(asked["text"], expected);
    let owner = format!(
        "The owner may approve this exact content instead with `board answer {effect} approve`; then check again."
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("Next: change pr.body:"), "{hint}");
    assert!(hint.ends_with(&owner), "{hint}");
    let (ran, again) = personal.call("check", &request(&failing()));
    assert_eq!(ran.code, 1);
    assert_eq!(again["fields"]["effect"], effect);
    assert_eq!(personal.pending(), [effect]);
}

#[test]
fn a_passing_check_asks_the_owner_nothing() {
    let personal = Personal::new();
    covered(&personal);
    let (ran, document) = personal.call("check", &request(&json!({"pr.body": "See here."})));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    assert_eq!(output(&document)["override"], Value::Null);
    assert_eq!(personal.pending(), Vec::<String>::new());
    let crossing = personal
        .record(output(&document)["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(crossing.get("override"), None);
}

#[test]
fn an_approved_override_passes_that_exact_content_and_records_the_effect() {
    let personal = Personal::new();
    covered(&personal);
    let (_, refused) = personal.call("check", &request(&failing()));
    let crossing = refused["fields"]["crossing"].as_str().unwrap();
    let effect = refused["fields"]["effect"].as_str().unwrap();
    personal.answer(effect, "approve");
    let (ran, document) = personal.call("check", &request(&failing()));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let result = output(&document);
    assert_eq!(
        (&result["crossing"], &result["override"]),
        (&json!(crossing), &json!(effect))
    );
    let statuses: Vec<(&Value, &Value)> = (result["verdicts"].as_array().unwrap().iter())
        .map(|verdict| (&verdict["part"], &verdict["status"]))
        .collect();
    assert_eq!(
        statuses,
        [
            (&json!("pr.body"), &json!("failed")),
            (&json!("pr.title"), &json!("passed"))
        ]
    );
    let record = personal.record(crossing).unwrap();
    assert_eq!(
        (&record["passed"], &record["override"]),
        (&json!(true), &json!(effect))
    );
    let digest = result["digest"].as_str().unwrap();
    let verified = json(&personal.run(&["--json", "call", "boundary.verify", "--digest", digest]));
    assert_eq!(output(&verified)["passed"], true);
    let recorded = personal.run(&[
        "--json",
        "call",
        "boundary.record",
        "--digest",
        digest,
        "--result",
        "u",
    ]);
    assert_eq!(recorded.code, 0, "{}", printed(&recorded));
}

#[test]
fn an_approval_does_not_let_through_content_that_differs_by_one_character() {
    let personal = Personal::new();
    covered(&personal);
    let (_, refused) = personal.call("check", &request(&failing()));
    personal.answer(refused["fields"]["effect"].as_str().unwrap(), "approve");
    let changed = record(failing(), &json!({"pr.title": "Fine."}));
    let (ran, document) =
        personal.call("check", &request(&serde_json::from_str(&changed).unwrap()));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    assert_ne!(
        document["fields"]["crossing"],
        refused["fields"]["crossing"]
    );
    assert_ne!(document["fields"]["effect"], refused["fields"]["effect"]);
    assert_eq!(
        personal.pending(),
        [document["fields"]["effect"].as_str().unwrap()]
    );
}

#[test]
fn an_override_the_owner_refused_stays_refused_and_does_not_offer_approval() {
    let personal = Personal::new();
    covered(&personal);
    let (_, asked) = personal.call("check", &request(&failing()));
    let effect = asked["fields"]["effect"].as_str().unwrap();
    personal.answer(effect, "refuse");
    let (ran, document) = personal.call("check", &request(&failing()));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    assert_eq!(document["fields"]["effect"], effect);
    let refused = format!(
        "The owner refused to let this exact content through ({effect}); the owner can ask again \
         by deleting /effects/seen/{effect} in the personal board."
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.ends_with(&refused), "{hint}");
    assert!(!printed(&ran).contains("board answer"));
    let record = personal
        .record(document["fields"]["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(record["passed"], false);
    assert_eq!(record.get("override"), None);
}

#[test]
fn failing_content_too_large_to_show_the_owner_still_fails_with_its_findings_and_no_effect() {
    let personal = Personal::new();
    covered(&personal);
    let large = format!("acme-123\n{}", "x".repeat(1 << 20));
    let (ran, document) = personal.call("check", &request(&json!({"pr.body": large})));
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::check::failed");
    let fields = &document["fields"];
    assert_eq!(
        fields["failures"],
        json!([{"control": "/controls/names", "part": "pr.body", "text": "acme-123", "line": 1}])
    );
    assert_eq!(fields.get("effect"), None);
    let ending = "This content is over 1 MiB as the owner would see it, so the owner cannot approve it; shorten it.";
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("Next: change pr.body:"), "{hint}");
    assert!(hint.ends_with(ending), "{hint}");
    assert_eq!(personal.pending(), Vec::<String>::new());
    let record = personal
        .record(fields["crossing"].as_str().unwrap())
        .unwrap();
    assert_eq!(record["passed"], false);
}
