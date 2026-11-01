//! `boundary.verify` finds a passing crossing, and `boundary.record` adds
//! what the destination returned to one.

use super::*;

const URL: &str = "https://github.com/octo/board/pull/7";

/// A pull request to octo/board from no checkout, with `body`.
fn request(body: &str) -> Value {
    json!({"action": "github.pr-create", "to": "github:octo/board", "parts": {"pr.body": body}})
}

/// Checks a pull request body against a boundary that refuses private
/// names, or `names` when given, and returns its digest.
fn checked(personal: &Personal, body: &str, names: Option<&str>) -> String {
    private_names(personal);
    if let Some(names) = names {
        personal.write("private/names.txt", &format!("{names}\n"));
    }
    let fields = json!({"to": "github:octo/*", "actions": ["github.*"], "checks": names_check()});
    personal.write("boundaries/octo.json", &boundary(&fields));
    let (_, document) = personal.call("check", &request(body));
    let digest = (document.get("fields").unwrap_or_else(|| output(&document)))["digest"].clone();
    digest.as_str().unwrap().to_owned()
}

/// `board --json call boundary.verify` with `flags`, and its output.
fn verify(personal: &Personal, flags: &[&str]) -> Value {
    let arguments: Vec<&str> = ["--json", "call", "boundary.verify"]
        .into_iter()
        .chain(flags.iter().copied())
        .collect();
    let ran = personal.run(&arguments);
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    output(&json(&ran)).clone()
}

#[test]
fn a_passing_crossing_verifies_by_digest_and_by_its_content() {
    let personal = Personal::new();
    let digest = checked(&personal, "Fine.", None);
    let expected = json!({"digest": digest, "crossing": format!("/crossings/{digest}"), "passed": true, "result": null});
    assert_eq!(verify(&personal, &["--digest", &digest]), expected);
    let (_, document) = personal.call("verify", &request("Fine."));
    assert_eq!(output(&document), &expected);
}

#[test]
fn a_crossing_checked_before_a_boundary_now_covers_it_does_not_verify() {
    let personal = Personal::new();
    let (_, document) = personal.call("check", &request("Fine."));
    let digest = output(&document)["digest"].as_str().unwrap().to_owned();
    assert_eq!(verify(&personal, &["--digest", &digest])["passed"], true);
    private_names(&personal);
    let fields = |to: &str| json!({"to": to, "actions": ["github.*"], "checks": names_check()});
    personal.write(
        "boundaries/other.json",
        &boundary(&fields("github:other/*")),
    );
    assert_eq!(verify(&personal, &["--digest", &digest])["passed"], true);
    personal.write("boundaries/octo.json", &boundary(&fields("github:octo/*")));
    assert_eq!(verify(&personal, &["--digest", &digest])["passed"], false);
}

#[test]
fn a_failed_crossing_and_a_missing_one_do_not_verify() {
    let personal = Personal::new();
    let digest = checked(&personal, "acme-123", None);
    assert_eq!(verify(&personal, &["--digest", &digest])["passed"], false);
    let (_, document) = personal.call("verify", &request("Never checked."));
    assert_eq!(output(&document)["crossing"], Value::Null);
    assert_eq!(output(&document)["passed"], false);
}

#[test]
fn verify_refuses_a_digest_given_with_content_and_takes_either_alone() {
    let personal = Personal::new();
    let digest = checked(&personal, "Fine.", None);
    let mut both = request("Fine.");
    both["digest"] = json!(digest);
    let (ran, document) = personal.call("verify", &both);
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::verify::bad_input");
    assert_eq!(
        document["fields"]["given"],
        json!(["action", "digest", "parts", "to"])
    );
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert_eq!(
        hint,
        format!("run `board --personal call boundary.verify --digest {digest}`")
    );
    let partial = json!({"to": "github:octo/board", "parts": {"pr.body": "Fine."}});
    let (_, document) = personal.call("verify", &partial);
    assert_eq!(document["code"], "boundary::verify::bad_input");
    let hint = document["recovery"]["text"].as_str().unwrap();
    assert_eq!(
        hint,
        "pass all of action, to and parts on stdin, or only --digest"
    );
    let (ran, _) = personal.call("verify", &json!({"digest": digest}));
    assert_eq!(ran.code, 0, "{}", printed(&ran));
}

/// `board --json call boundary.record` for `digest` with [`URL`] as its
/// result, and its one document.
fn record_url(personal: &Personal, digest: &str) -> (Output, Value) {
    let ran = personal.run(&[
        "--json",
        "call",
        "boundary.record",
        "--digest",
        digest,
        "--result",
        URL,
    ]);
    let document = json(&ran);
    (ran, document)
}

#[test]
fn record_adds_the_result_to_a_passing_crossing_and_verify_reports_it() {
    let personal = Personal::new();
    let digest = checked(&personal, "Fine.", None);
    let (ran, document) = record_url(&personal, &digest);
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    let path = format!("/crossings/{digest}");
    assert_eq!(output(&document), &json!({"crossing": path, "result": URL}));
    let crossing = personal.record(&path).unwrap();
    assert_eq!(
        (&crossing["result"], &crossing["passed"]),
        (&json!(URL), &json!(true))
    );
    assert_eq!(verify(&personal, &["--digest", &digest])["result"], URL);
}

#[test]
fn a_passing_recheck_keeps_the_recorded_result_and_a_failing_one_drops_it() {
    let personal = Personal::new();
    let digest = checked(&personal, "Fine.", None);
    let (ran, _) = record_url(&personal, &digest);
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    assert_eq!(checked(&personal, "Fine.", None), digest);
    assert_eq!(verify(&personal, &["--digest", &digest])["result"], URL);
    assert_eq!(checked(&personal, "Fine.", Some("Fine")), digest);
    assert_eq!(
        verify(&personal, &["--digest", &digest]),
        json!({"digest": digest, "crossing": format!("/crossings/{digest}"), "passed": false, "result": null})
    );
}

#[test]
fn record_refuses_a_failed_crossing_and_one_never_checked() {
    let personal = Personal::new();
    let failed = checked(&personal, "acme-123", None);
    let (ran, document) = record_url(&personal, &failed);
    assert_eq!(ran.code, 1);
    assert_eq!(document["code"], "boundary::crossing::failed");
    let crossing = personal.record(&format!("/crossings/{failed}")).unwrap();
    assert_eq!(crossing.get("result"), None);
    let (_, document) = record_url(&personal, &"0".repeat(64));
    assert_eq!(document["code"], "boundary::crossing::not_found");
    let passed = checked(&personal, "Fine.", None);
    let (ran, _) = record_url(&personal, &passed);
    assert_eq!(ran.code, 0, "{}", printed(&ran));
}
