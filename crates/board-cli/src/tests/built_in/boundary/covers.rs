//! `boundary.covers` names the boundaries a check would apply, by the same
//! rules, and checks nothing.

use super::*;

/// `board --json call boundary.covers` with each field of `input` as a
/// flag, since every input of `covers` is one, and its one document.
fn call(personal: &Personal, input: &Value) -> (Output, Value) {
    let flags: Vec<String> = (input.as_object().unwrap().iter())
        .flat_map(|(name, value)| [format!("--{name}"), value.as_str().unwrap().to_owned()])
        .collect();
    let arguments: Vec<&str> = ["--json", "call", "boundary.covers"]
        .into_iter()
        .chain(flags.iter().map(String::as_str))
        .collect();
    let ran = personal.run(&arguments);
    let document = json(&ran);
    (ran, document)
}

/// The boundaries `boundary.covers` names for `input`.
fn covered(personal: &Personal, input: &Value) -> Value {
    let (ran, document) = call(personal, input);
    assert_eq!(ran.code, 0, "{}", printed(&ran));
    output(&document)["boundaries"].clone()
}

#[test]
fn a_boundary_covers_its_destination_and_actions_and_not_another() {
    let personal = Personal::new();
    private_names(&personal);
    let fields =
        json!({"to": "github:Octo/*", "actions": ["github.pr-*"], "checks": names_check()});
    personal.write("boundaries/octo.json", &boundary(&fields));
    let asked = |action: &str, to: &str| json!({"action": action, "to": to});
    assert_eq!(
        covered(&personal, &asked("github.pr-create", "github:octo/widgets")),
        json!(["/boundaries/octo"])
    );
    assert_eq!(
        covered(&personal, &asked("github.push", "github:octo/widgets")),
        json!([])
    );
    assert_eq!(
        covered(
            &personal,
            &asked("github.pr-create", "github:other/widgets")
        ),
        json!([])
    );
    assert!(!personal.board.workdir().join("crossings").exists());
}

#[test]
fn a_boundary_limited_by_from_covers_a_checkout_under_it_and_one_that_names_no_checkout() {
    let personal = Personal::new();
    private_names(&personal);
    let fields = json!({
        "to": "github:*",
        "actions": ["github.*"],
        "checks": names_check(),
        "from": ["/repos/a"],
    });
    personal.write("boundaries/octo.json", &boundary(&fields));
    let given = |from: Option<&str>| {
        let mut given = json!({"action": "github.push", "to": "github:octo/widgets"});
        if let Some(from) = from {
            given["from"] = json!(from);
        }
        given
    };
    assert_eq!(
        covered(&personal, &given(Some("/repos/a/sub"))),
        json!(["/boundaries/octo"])
    );
    assert_eq!(
        covered(&personal, &given(None)),
        json!(["/boundaries/octo"])
    );
    assert_eq!(covered(&personal, &given(Some("/repos/b"))), json!([]));
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test removes the broken boundary it wrote"
)]
fn an_invalid_boundary_refuses_covers_as_it_refuses_check() {
    let personal = Personal::new();
    personal.write("boundaries/broken.json", "{");
    let input = json!({"action": "github.push", "to": "github:octo/widgets"});
    let (ran, document) = call(&personal, &input);
    assert_eq!(
        (ran.code, &document["code"]),
        (1, &json!("boundary::boundary::invalid"))
    );
    std::fs::remove_file(personal.board.workdir().join("boundaries/broken.json")).unwrap();
    assert_eq!(covered(&personal, &input), json!([]));
}
