//! `controls.from-standards` through the binary: the controls a standards
//! repository's rule tables give an adopter, as lines `board put --batch`
//! applies, which `controls.for-paths` then finds.

use super::*;

const BASE: &str = "# base\n\n## lint\n\n| ID | Rule | Check |\n| --- | --- | --- |\n\
    | BASE-1 | Lint. | `x` |\n\n## review\n\n\
    | ID | Rule | Where it applies |\n| --- | --- | --- |\n\
    | BASE-31 | Give each meaning one type. Prefer a library type. | `**/*.rs` |\n\
    | BASE-53 | Write the test first. | `**` |\n";

const SERVICE: &str = "# service\n\n## review\n\n\
    | ID | Rule | Where it applies |\n| --- | --- | --- |\n\
    | SVC-9 | Log through tracing. | `crates/*-server/**` |\n";

/// A board that mounts a standards repository at /repos/standards and an
/// adopter at /repos/app whose map adopts `profiles`. The repositories live
/// as long as the values returned.
fn adopting(profiles: &str) -> (Fixture, Scenario, Scenario) {
    let board = board();
    let map = format!("schema = \"rust-standards/adoption/v1\"\nprofiles = {profiles}\n");
    let standards = library(
        &board,
        "repos/standards.json",
        &[("profiles/base.md", BASE), ("profiles/service.md", SERVICE)],
    );
    let app = library(
        &board,
        "repos/app.json",
        &[
            ("rust-standards.toml", map.as_str()),
            ("crates/app-server/src/main.rs", "//! M.\n"),
            ("web/app.ts", "x\n"),
        ],
    );
    (board, standards.0, app.0)
}

/// The run of `controls.from-standards` for the adopter, into
/// /controls/app/standards, and its one document.
fn from_standards(board: &Fixture) -> (Output, Value) {
    let ran = board.run(&[
        "--json",
        "call",
        "controls.from-standards",
        "--standards",
        "/repos/standards",
        "--map",
        "/repos/app/rust-standards.toml",
        "--into",
        "/controls/app/standards",
    ]);
    let document = json(&ran);
    (ran, document)
}

/// The output of a `controls.from-standards` run that succeeds.
fn generate(board: &Fixture) -> Value {
    let (ran, document) = from_standards(board);
    assert_eq!(ran.code, 0, "{document}");
    document["data"]["output"].clone()
}

/// Applies `lines` with `board put --batch`.
fn apply(board: &Fixture, lines: &Value) {
    let mut batch = String::new();
    for line in lines.as_array().unwrap() {
        batch.push_str(&line.to_string());
        batch.push('\n');
    }
    let put = board.run_with(&["put", "--batch"], &batch);
    assert_eq!(put.code, 0, "{}", text(&put.stderr));
}

#[test]
fn each_rule_of_a_table_with_where_it_applies_becomes_a_control_anchored_at_the_adopter() {
    let (board, _standards, _app) = adopting(r#"["base", "service"]"#);
    let output = generate(&board);
    assert_eq!(
        output["profiles"],
        json!([
            {"name": "base", "file": "/repos/standards/profiles/base.md", "rules": 2},
            {"name": "service", "file": "/repos/standards/profiles/service.md", "rules": 1},
        ])
    );
    let lines = output["lines"].as_array().unwrap();
    let paths: Vec<&str> = lines.iter().map(|l| l["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "/controls/app/standards/base-31",
            "/controls/app/standards/base-53",
            "/controls/app/standards/svc-9",
        ]
    );
    assert_eq!(
        lines[0],
        json!({
            "path": "/controls/app/standards/base-31",
            "expect": "none",
            "record": {
                "$type": "controls/control/v1",
                "requirement": "Give each meaning one type. Prefer a library type.",
                "purpose": "BASE-31 is a rule of the base profile of the standards at \
                    /repos/standards, which /repos/app/rust-standards.toml adopts.",
                "summary": "Give each meaning one type.",
                "paths": ["repos/app/*.rs", "repos/app/*/*.rs"],
                "sources": ["/repos/standards/profiles/base.md"],
            },
        })
    );
    apply(&board, &output["lines"]);
    assert_eq!(
        board.checked("/controls/app"),
        codes(&[
            ("/controls/app/standards/base-31", None),
            ("/controls/app/standards/base-53", None),
            ("/controls/app/standards/svc-9", None),
        ])
    );
    let found = board.call("controls.for-path", &["--path", "/repos/app/web/app.ts"]);
    let controls: Vec<&str> = (found["controls"].as_array().unwrap().iter())
        .map(|c| c["control"].as_str().unwrap())
        .collect();
    assert_eq!(controls, ["/controls/app/standards/base-53"]);
}

#[test]
fn generating_again_expects_what_is_held_and_removes_a_control_no_profile_gives() {
    let (board, _standards, _app) = adopting(r#"["base", "service"]"#);
    apply(&board, &generate(&board)["lines"]);
    let rewritten = r#"schema = "rust-standards/adoption/v1"
profiles = ["base"]
"#;
    let (app, commit) = {
        let app = Scenario::new();
        let commit = app.commit(&[
            ("rust-standards.toml", rewritten),
            ("crates/app-server/src/main.rs", "//! M.\n"),
        ]);
        (app, commit)
    };
    mount(&board, "repos/app.json", &app, commit);
    let lines = generate(&board)["lines"].clone();
    let removed: Vec<&Value> = (lines.as_array().unwrap().iter())
        .filter(|line| line.get("remove").is_some())
        .collect();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["path"], json!("/controls/app/standards/svc-9"));
    assert_ne!(removed[0]["expect"], json!("none"));
    apply(&board, &lines);
    assert_eq!(board.checked("/controls/app").len(), 2);
}

#[test]
fn a_map_without_a_profiles_list_is_refused_and_one_with_it_is_read() {
    let (board, _standards, _app) = adopting(r#""base""#);
    let (ran, document) = from_standards(&board);
    assert_ne!(ran.code, 0);
    assert_eq!(
        document["code"],
        json!("controls::standards::map_invalid"),
        "{document}"
    );
    let (board, _standards, _app) = adopting(r#"["base"]"#);
    assert_eq!(generate(&board)["profiles"][0]["rules"], json!(2));
}

#[test]
fn a_profile_the_standards_do_not_hold_is_refused_and_one_they_hold_is_read() {
    let (board, _standards, _app) = adopting(r#"["base", "gone"]"#);
    let (ran, document) = from_standards(&board);
    assert_ne!(ran.code, 0);
    assert_eq!(
        document["code"],
        json!("controls::standards::unreadable"),
        "{document}"
    );
    assert_eq!(
        document["fields"]["path"],
        json!("/repos/standards/profiles/gone.md")
    );
    let (board, _standards, _app) = adopting(r#"["base", "service"]"#);
    assert_eq!(generate(&board)["profiles"][1]["rules"], json!(1));
}
