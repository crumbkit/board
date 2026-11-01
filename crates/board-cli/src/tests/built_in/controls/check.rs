//! `controls.check` through the binary: what each budget measures, which
//! controls a path reports, and when the check fails.

use super::*;

const WORDS: u64 = 3;
const BOTH: u64 = 5;
const EXCEEDED: &str = "controls::budget::exceeded";
const UNREADABLE: &str = "controls::file::unreadable";

/// A budget of `max` words.
fn words(max: u64) -> Value {
    json!({"budget": "words", "max": max})
}

/// A control with `requirement` on the files `paths` match, under `rule`.
fn budget(requirement: &str, paths: &[&str], rule: &Value) -> String {
    control(requirement, &json!({"paths": paths, "rule": rule}))
}

/// The output of `controls.check` with `flags`.
fn check(board: &Fixture, flags: &[&str]) -> Value {
    board.call("controls.check", flags)
}

/// The output of `controls.check` at `path`.
fn at(board: &Fixture, path: &str) -> Value {
    check(board, &["--path", path])
}

/// The `field` of each measured control of `output`.
fn each(output: &Value, field: &str) -> Vec<Value> {
    (output["measured"].as_array().unwrap().iter())
        .map(|item| item[field].clone())
        .collect()
}

/// The control, files and words used of each measured control of `output`.
fn triples(output: &Value) -> Vec<Value> {
    (output["measured"].as_array().unwrap().iter())
        .map(|item| json!([item["control"], item["files"], item["used"]]))
        .collect()
}

/// The measured and the stated controls of a `check` output, by path.
fn names(output: &Value) -> (Vec<String>, Vec<String>) {
    let paths = |list: &Value| {
        (list.as_array().unwrap().iter())
            .map(|item| item["control"].as_str().unwrap().to_owned())
            .collect()
    };
    (paths(&output["measured"]), paths(&output["stated"]))
}

/// What `names` gives for `measured` and `stated`.
fn named(measured: &[&str], stated: &[&str]) -> (Vec<String>, Vec<String>) {
    let owned = |list: &[&str]| list.iter().map(|path| (*path).to_owned()).collect();
    (owned(measured), owned(stated))
}

#[test]
fn check_measures_each_budget_control_over_its_files() {
    let board = board();
    board
        .write("AGENTS.md", "one two\nthree\n")
        .write("docs/a.md", "four five")
        .write("docs/b.md", "six")
        .write(
            "controls/context.json",
            &control(
                "Short.",
                &json!({"applies_to": ["/AGENTS.md"], "rule": words(3)}),
            ),
        )
        .write(
            "controls/docs.json",
            &control(
                "Small.",
                &json!({
                    "applies_to": ["/docs/a.md"],
                    "paths": ["docs/*.md"],
                    "rule": {"budget": "bytes", "max": 12},
                }),
            ),
        )
        .write(
            "controls/plain.json",
            &control("No budget.", &json!({"paths": ["*"]})),
        );
    assert_eq!(
        check(&board, &[])["measured"],
        json!([
            {
                "control": "/controls/context",
                "requirement": "Short.",
                "budget": "words",
                "max": 3,
                "used": 3,
                "files": ["/AGENTS.md"],
            },
            {
                "control": "/controls/docs",
                "requirement": "Small.",
                "budget": "bytes",
                "max": 12,
                "used": 12,
                "files": ["/docs/a.md", "/docs/b.md"],
            },
        ])
    );
}

/// A board whose one control allows `max` words of a three-word file.
fn over(board: &Fixture, max: u64) {
    board.write("AGENTS.md", "one two three").write(
        "controls/context.json",
        &control(
            "Short.",
            &json!({"applies_to": ["/AGENTS.md"], "rule": words(max)}),
        ),
    );
}

#[test]
fn a_control_over_its_budget_fails_the_check_and_one_at_it_passes() {
    let board = board();
    over(&board, 2);
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], EXCEEDED, "{refused}");
    assert!(
        (refused["message"].as_str().unwrap()).contains("/controls/context (3 of 2 words)"),
        "{refused}"
    );
    assert_eq!(refused["fields"]["controls"][0]["used"], WORDS);
    over(&board, WORDS);
    assert_eq!(each(&check(&board, &[]), "used"), [json!(WORDS)]);
}

#[test]
fn a_control_whose_file_is_missing_fails_the_check_and_one_whose_file_exists_passes() {
    let board = board();
    board.write(
        "controls/context.json",
        &control(
            "Short.",
            &json!({"applies_to": ["/AGENTS.md"], "rule": {"budget": "bytes", "max": 9}}),
        ),
    );
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], "controls::control::degraded", "{refused}");
    assert_eq!(
        refused["fields"]["controls"],
        json!([{"control": "/controls/context", "code": "board::ref::dangling"}])
    );
    board.write("AGENTS.md", "x");
    assert_eq!(each(&check(&board, &[]), "used"), [json!(1)]);
}

#[test]
fn a_file_reached_through_a_link_and_by_pattern_counts_once_and_a_link_counts_nothing() {
    let board = board();
    board
        .write("docs/a.md", "one two three")
        .write(
            "alias.json",
            r#"{"$type": "board/mount/v1", "path": "/docs"}"#,
        )
        .write(
            "controls/docs.json",
            &control(
                "Small.",
                &json!({
                    "applies_to": ["/alias/a.md"],
                    "paths": ["docs/*", "alias*"],
                    "rule": words(WORDS),
                }),
            ),
        );
    assert_eq!(
        triples(&check(&board, &[])),
        [json!(["/controls/docs", ["/docs/a.md"], WORDS])]
    );
}

#[test]
fn a_pattern_reaching_into_a_mount_counts_files_there_and_one_that_does_not_counts_none() {
    let board = board();
    let _library = library(&board, "vendor/lib.json", &[("docs/a.md", "one two three")]);
    board
        .write(
            "controls/vendor.json",
            &budget("Small.", &["vendor/*.md"], &words(WORDS)),
        )
        .write(
            "controls/own.json",
            &budget("Small.", &["docs/*.md"], &words(WORDS)),
        );
    assert_eq!(
        triples(&check(&board, &[])),
        [
            json!(["/controls/own", [], 0]),
            json!(["/controls/vendor", ["/vendor/lib/docs/a.md"], WORDS]),
        ]
    );
}

#[test]
fn a_malformed_path_in_a_mount_matches_nothing_and_a_well_formed_one_beside_it_counts() {
    let board = board();
    let _library = library(
        &board,
        "vendor/lib.json",
        &[("docs/\u{1}.md", "one"), ("docs/a.md", "two")],
    );
    board.write(
        "controls/vendor.json",
        &budget("Small.", &["vendor/*"], &words(WORDS)),
    );
    assert_eq!(
        each(&check(&board, &[]), "files"),
        [json!(["/vendor/lib/docs/a.md"])]
    );
}

#[test]
fn a_pattern_matching_a_degraded_file_fails_the_check_and_one_matching_a_whole_file_passes() {
    let board = board();
    board
        .write("docs/a", "one")
        .write("docs/a.json", "{}")
        .write(
            "controls/docs.json",
            &budget("Small.", &["docs/*"], &words(WORDS)),
        );
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], UNREADABLE, "{refused}");
    assert_eq!(refused["fields"]["control"], "/controls/docs");
    assert_eq!(refused["fields"]["code"], "board::path::shadowed");
    board.delete("docs/a.json");
    assert_eq!(each(&check(&board, &[]), "used"), [json!(1)]);
}

#[test]
fn a_pattern_matching_a_file_with_no_logical_path_fails_naming_its_path_and_one_with_a_logical_path_passes()
 {
    let board = board();
    board.write("docs/\u{1}.md", "one").write(
        "controls/docs.json",
        &budget("Small.", &["docs/*"], &words(WORDS)),
    );
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], UNREADABLE, "{refused}");
    assert_eq!(refused["fields"]["file"], "docs/\u{1}.md");
    assert_eq!(refused["fields"]["code"], "board::path::malformed");
    board.delete("docs/\u{1}.md");
    board.write("docs/a.md", "one");
    assert_eq!(each(&check(&board, &[]), "files"), [json!(["/docs/a.md"])]);
}

#[test]
fn a_reference_to_a_directory_fails_the_check_and_one_to_a_file_passes() {
    let board = board();
    let referring = |file: &str| {
        control(
            "Small.",
            &json!({"applies_to": [file], "rule": words(WORDS)}),
        )
    };
    board
        .write("docs/a.md", "one")
        .write("controls/docs.json", &referring("/docs"));
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], UNREADABLE, "{refused}");
    assert_eq!(refused["fields"]["file"], "/docs");
    assert!(
        !refused["message"]
            .as_str()
            .unwrap()
            .contains("first on PATH"),
        "{refused}"
    );
    board.write("controls/docs.json", &referring("/docs/a.md"));
    assert_eq!(each(&check(&board, &[]), "used"), [json!(1)]);
}

#[test]
fn a_control_naming_a_directory_fails_naming_it_and_boards_code_and_one_naming_a_file_passes() {
    let board = board();
    let referring = |file: &str| {
        control(
            "Small.",
            &json!({"applies_to": [file], "rule": words(WORDS)}),
        )
    };
    board
        .write("adir/a.md", "one")
        .write("zdir/z.md", "one")
        .write("controls/a.json", &referring("/zdir"))
        .write("controls/b.json", &referring("/adir/a.md"));
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], UNREADABLE, "{refused}");
    let fields = &refused["fields"];
    assert_eq!(
        (&fields["control"], &fields["file"], &fields["code"]),
        (
            &json!("/controls/a"),
            &json!("/zdir"),
            &json!("board::record::not_a_record")
        )
    );
    board.write("controls/a.json", &referring("/zdir/z.md"));
    assert_eq!(each(&check(&board, &[]), "used"), [json!(1), json!(1)]);
}

/// A board with a budget and a stated control under /docs, the same two
/// under /src, and one on a mount at /vendor/lib, which lives as long as the
/// value returned.
fn scoped() -> (Fixture, Scenario) {
    let board = board();
    let (library, _) = library(&board, "vendor/lib.json", &[("README.md", "four")]);
    board
        .write("docs/a.md", "one two")
        .write("src/a.rs", "three")
        .write(
            "alias.json",
            r#"{"$type": "board/mount/v1", "path": "/docs"}"#,
        )
        .write(
            "controls/docs-budget.json",
            &budget("Short docs.", &["docs/*"], &words(WORDS)),
        )
        .write(
            "controls/docs-style.json",
            &control("Plain docs.", &json!({"applies_to": ["/docs/a.md"]})),
        )
        .write(
            "controls/src-budget.json",
            &budget("Short code.", &["src/*"], &words(WORDS)),
        )
        .write(
            "controls/src-style.json",
            &control("Plain code.", &json!({"paths": ["src/*"]})),
        )
        .write(
            "controls/vendor.json",
            &control("Leave vendored files.", &json!({"paths": ["vendor/*"]})),
        );
    (board, library)
}

/// What `check` at /docs reports on the `scoped` board.
fn docs() -> (Vec<String>, Vec<String>) {
    named(&["/controls/docs-budget"], &["/controls/docs-style"])
}

#[test]
fn a_path_reports_the_controls_under_it_and_leaves_out_their_twins_elsewhere() {
    let (board, _library) = scoped();
    let output = at(&board, "/docs");
    assert_eq!(names(&output), docs());
    assert_eq!(
        output["stated"],
        json!([{
            "control": "/controls/docs-style",
            "requirement": "Plain docs.",
            "purpose": "A test.",
            "paths": [],
            "files": 1,
        }])
    );
    assert_eq!(each(&output, "used"), [json!(2)], "the words in docs/a.md");
    assert_eq!(
        names(&at(&board, "/src")),
        named(&["/controls/src-budget"], &["/controls/src-style"])
    );
}

#[test]
fn check_without_a_path_reports_every_control_as_the_root_does() {
    let (board, _library) = scoped();
    let everything = named(
        &["/controls/docs-budget", "/controls/src-budget"],
        &[
            "/controls/docs-style",
            "/controls/src-style",
            "/controls/vendor",
        ],
    );
    assert_eq!(names(&check(&board, &[])), everything);
    assert_eq!(names(&at(&board, "/")), everything);
}

#[test]
fn a_path_into_a_mount_reports_its_controls_and_one_beside_the_mount_does_not() {
    let (board, _library) = scoped();
    let output = at(&board, "/vendor/lib");
    assert_eq!(names(&output), named(&[], &["/controls/vendor"]));
    assert_eq!(output["stated"][0]["files"], 1);
    assert_eq!(
        names(&at(&board, "/vendor")),
        named(&[], &["/controls/vendor"])
    );
    assert_eq!(names(&at(&board, "/controls")), named(&[], &[]));
}

#[test]
fn a_path_through_a_link_is_checked_where_it_leads() {
    let (board, _library) = scoped();
    assert_eq!(names(&at(&board, "/alias")), names(&at(&board, "/docs")));
}

#[test]
fn a_path_with_nothing_there_is_refused_and_one_with_a_file_there_is_admitted() {
    let (board, _library) = scoped();
    let refused = board.refusal("controls.check", &["--path", "/nowhere"]);
    assert_eq!(refused["code"], "controls::board::refused", "{refused}");
    assert_eq!(refused["fields"]["code"], "board::path::not_found");
    assert!(
        (refused["message"].as_str().unwrap()).contains("nothing is at /nowhere"),
        "{refused}"
    );
    assert_eq!(names(&at(&board, "/docs/a.md")), docs());
}

#[test]
fn a_budget_over_its_max_fails_a_path_it_applies_under_and_not_one_it_does_not() {
    let (board, _library) = scoped();
    board.write("src/b.rs", "five six seven");
    let ran = board.run(&["--json", "call", "controls.check", "--path", "/src"]);
    let refused = json(&ran);
    assert_eq!(ran.code, 1, "{refused}");
    assert_eq!(refused["code"], EXCEEDED, "{refused}");
    let failing: Vec<&Value> = (refused["fields"]["controls"].as_array().unwrap().iter())
        .map(|item| &item["control"])
        .collect();
    assert_eq!(failing, [&json!("/controls/src-budget")]);
    assert_eq!(names(&at(&board, "/docs")), docs());
}

#[test]
fn a_degraded_control_fails_a_path_it_does_not_reach_and_the_same_control_whole_passes() {
    let (board, _library) = scoped();
    board.write(
        "controls/src-file.json",
        &control("Plain code.", &json!({"applies_to": ["/src/b.rs"]})),
    );
    let refused = board.refusal("controls.check", &["--path", "/docs"]);
    assert_eq!(refused["code"], "controls::control::degraded", "{refused}");
    assert_eq!(
        refused["fields"]["controls"],
        json!([{"control": "/controls/src-file", "code": "board::ref::dangling"}])
    );
    board.write("src/b.rs", "five");
    assert_eq!(names(&at(&board, "/docs")), docs());
}

#[test]
fn a_stated_control_reaching_an_unreadable_file_passes_and_a_budget_reaching_it_fails() {
    let board = board();
    board
        .write("docs/a", "one")
        .write("docs/a.json", "{}")
        .write(
            "controls/style.json",
            &control("Plain.", &json!({"paths": ["docs/*"]})),
        );
    assert_eq!(
        names(&at(&board, "/docs")),
        named(&[], &["/controls/style"])
    );
    board.write(
        "controls/budget.json",
        &budget("Small.", &["docs/*"], &words(WORDS)),
    );
    let refused = board.refusal("controls.check", &["--path", "/docs"]);
    assert_eq!(refused["code"], UNREADABLE, "{refused}");
    assert_eq!(refused["fields"]["control"], "/controls/budget");
}

/// The `fields` of the item of `output`'s `list` that is `control`.
fn of(output: &Value, list: &str, control: &str, fields: &[&str]) -> Vec<Value> {
    (output[list].as_array().unwrap().iter())
        .filter(|item| item["control"] == control)
        .map(|item| fields.iter().map(|field| item[*field].clone()).collect())
        .collect()
}

#[test]
fn a_budget_lists_its_files_outside_the_path_and_a_stated_control_counts_only_those_inside() {
    let (board, _library) = scoped();
    board
        .write(
            "controls/both.json",
            &budget("Both.", &["docs/*", "src/*"], &words(9)),
        )
        .write(
            "controls/both-style.json",
            &control("Both.", &json!({"paths": ["docs/*", "src/*"]})),
        );
    let output = at(&board, "/docs");
    assert_eq!(
        of(&output, "measured", "/controls/both", &["files", "used"]),
        [json!([["/docs/a.md", "/src/a.rs"], WORDS])]
    );
    assert_eq!(
        of(
            &output,
            "stated",
            "/controls/both-style",
            &["paths", "files"]
        ),
        [json!([["docs/*", "src/*"], 1])]
    );
}

#[test]
fn the_root_lists_a_control_that_reaches_no_file_and_a_narrower_path_leaves_it_out() {
    let (board, _library) = scoped();
    board
        .write(
            "controls/typo.json",
            &budget("Typo.", &["dosc/*"], &words(WORDS)),
        )
        .write(
            "controls/typo-style.json",
            &control("Typo.", &json!({"paths": ["dosc/*"]})),
        );
    let output = check(&board, &[]);
    assert_eq!(
        triples(&output).last(),
        Some(&json!(["/controls/typo", [], 0]))
    );
    assert_eq!(
        of(&output, "stated", "/controls/typo-style", &["files"]),
        [json!([0])]
    );
    assert_eq!(names(&at(&board, "/docs")), docs());
}

#[test]
fn a_stated_control_counts_a_file_it_reaches_by_reference_and_pattern_once_and_another_file_again()
{
    let (board, _library) = scoped();
    board.write(
        "controls/twice.json",
        &control(
            "Twice.",
            &json!({"applies_to": ["/docs/a.md"], "paths": ["docs/*"]}),
        ),
    );
    let counted = || {
        of(
            &at(&board, "/docs"),
            "stated",
            "/controls/twice",
            &["paths", "files"],
        )
    };
    assert_eq!(counted(), [json!([["docs/*"], 1])]);
    board.write("docs/b.md", "eight");
    assert_eq!(counted(), [json!([["docs/*"], 2])]);
}

/// A board with a three-word and a two-word file, `BOTH` words in all,
/// under a control of `max` words `per` `mode`.
fn per(board: &Fixture, mode: Option<&str>, max: u64) {
    let mut rule = words(max);
    if let Some(mode) = mode {
        rule["per"] = json!(mode);
    }
    board
        .write("docs/a.md", "one two three")
        .write("docs/b.md", "four five")
        .write(
            "controls/docs.json",
            &budget("Small.", &["docs/*.md"], &rule),
        );
}

#[test]
fn a_per_file_control_passes_when_each_file_is_within_it_and_a_total_one_sums_the_same_files() {
    let board = board();
    per(&board, Some("file"), WORDS);
    assert_eq!(
        check(&board, &[])["measured"],
        json!([{
            "control": "/controls/docs",
            "requirement": "Small.",
            "budget": "words",
            "max": WORDS,
            "used": WORDS,
            "files": ["/docs/a.md", "/docs/b.md"],
            "per": "file",
            "largest": "/docs/a.md",
            "over": [],
        }])
    );
    per(&board, Some("total"), BOTH);
    let summed = check(&board, &[]);
    assert_eq!(each(&summed, "used"), [json!(BOTH)]);
    assert_eq!(
        each(&summed, "files"),
        [json!(["/docs/a.md", "/docs/b.md"])]
    );
    per(&board, Some("total"), WORDS);
    assert_eq!(board.refusal("controls.check", &[])["code"], EXCEEDED);
}

#[test]
fn a_per_file_control_fails_naming_each_file_over_it_and_passes_once_that_file_is_within_it() {
    let board = board();
    per(&board, Some("file"), 2);
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], EXCEEDED, "{refused}");
    assert!(
        (refused["message"].as_str().unwrap())
            .contains("/controls/docs (each file at most 2 words: /docs/a.md has 3)"),
        "{refused}"
    );
    let failing = &refused["fields"]["controls"];
    assert_eq!(failing.as_array().unwrap().len(), 1, "{failing}");
    assert_eq!(
        (&failing[0]["used"], &failing[0]["largest"]),
        (&json!(WORDS), &json!("/docs/a.md"))
    );
    assert_eq!(
        failing[0]["over"],
        json!([{"file": "/docs/a.md", "used": WORDS}])
    );
    board.write("docs/a.md", "one two");
    assert_eq!(each(&check(&board, &[]), "over"), [json!([])]);
}

#[test]
fn a_per_file_control_names_the_first_of_equal_files_as_largest_and_lists_every_file_over_it() {
    let board = board();
    per(&board, Some("file"), 2);
    board.write("docs/b.md", "four five six");
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], EXCEEDED, "{refused}");
    assert!(
        (refused["message"].as_str().unwrap()).contains(
            "/controls/docs (each file at most 2 words: /docs/a.md has 3, /docs/b.md has 3)"
        ),
        "{refused}"
    );
    let failing = &refused["fields"]["controls"];
    assert_eq!(failing.as_array().unwrap().len(), 1, "{failing}");
    assert_eq!(
        (&failing[0]["used"], &failing[0]["largest"]),
        (&json!(WORDS), &json!("/docs/a.md"))
    );
    assert_eq!(
        failing[0]["over"],
        json!([
            {"file": "/docs/a.md", "used": WORDS},
            {"file": "/docs/b.md", "used": WORDS},
        ])
    );
}

#[test]
fn a_control_naming_per_total_measures_exactly_as_one_naming_no_per() {
    let board = board();
    per(&board, Some("total"), BOTH);
    let named = check(&board, &[]);
    per(&board, None, BOTH);
    let unnamed = check(&board, &[]);
    assert_eq!(named, unnamed);
    let keys: Vec<&String> = unnamed["measured"][0].as_object().unwrap().keys().collect();
    let mut keys: Vec<&str> = keys.into_iter().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["budget", "control", "files", "max", "requirement", "used"]
    );
}

#[test]
fn a_per_file_control_with_no_files_measures_nothing_as_its_largest() {
    let board = board();
    board.write(
        "controls/docs.json",
        &budget(
            "Small.",
            &["docs/*.md"],
            &json!({"budget": "words", "max": 1, "per": "file"}),
        ),
    );
    let output = check(&board, &[]);
    let measured = &output["measured"];
    assert_eq!(measured.as_array().unwrap().len(), 1, "{measured}");
    assert_eq!(
        (
            &measured[0]["used"],
            &measured[0]["largest"],
            &measured[0]["over"]
        ),
        (&json!(0), &Value::Null, &json!([]))
    );
}

#[test]
fn a_control_at_a_malformed_path_fails_the_check_naming_its_file_and_its_twin_at_a_well_formed_path_passes()
 {
    let board = board();
    let malformed = "controls/bad\u{200b}name.json";
    let short = budget("Short.", &["*.md"], &words(WORDS));
    board
        .write("a.md", "one two three")
        .write(malformed, &short);
    let refused = board.refusal("controls.check", &[]);
    assert_eq!(refused["code"], "controls::control::degraded", "{refused}");
    assert_eq!(
        refused["fields"]["controls"],
        json!([{"control": malformed, "code": "board::path::malformed"}])
    );
    board.delete(malformed);
    board.write("controls/name.json", &short);
    assert_eq!(each(&check(&board, &[]), "used"), [json!(WORDS)]);
}

#[test]
fn a_pattern_or_judged_control_is_stated_and_a_budget_beside_it_is_measured() {
    let board = board();
    board
        .write("docs/a.md", "one two")
        .write("private/names.txt", "acme\n")
        .write("rubrics/plain.md", "Write plainly.\n")
        .write(
            "controls/names.json",
            &budget(
                "No names.",
                &["docs/*.md"],
                &json!({"pattern": "/private/names.txt", "show": "match-only"}),
            ),
        )
        .write(
            "controls/plain.json",
            &budget(
                "Plain.",
                &["docs/*.md"],
                &json!({"judged": "/rubrics/plain.md"}),
            ),
        )
        .write(
            "controls/short.json",
            &budget("Short.", &["docs/*.md"], &words(2)),
        );
    assert_eq!(
        names(&check(&board, &[])),
        named(
            &["/controls/short"],
            &["/controls/names", "/controls/plain"]
        )
    );
}
