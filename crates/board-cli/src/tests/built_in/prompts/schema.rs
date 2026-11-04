//! The schemas prompts ships, as `board plugin enable` writes them and
//! `board check` reads prompts with them. These were `test_schema.py` and
//! the enabling test of `test_manifest.py`.

use super::*;

#[test]
fn board_lists_the_plugin_as_enabled_and_enabling_wrote_the_schemas_it_ships() {
    let board = board();
    let report = board.json(&["plugin", "list"]);
    let listed = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "prompts")
        .unwrap()
        .clone();
    assert_eq!(
        (&listed["status"], &listed["enabled"], &listed["offers"]),
        (&json!("ok"), &json!(true), &json!(["get", "list"]))
    );
    for (path, text) in shipped("prompts") {
        assert_eq!(board.read(&path), text, "{path}");
    }
}

fn with_prompt(fields: &Value) -> String {
    prompt("Review a pull request", "", fields)
}

#[test]
fn a_prompt_with_its_required_fields_and_inputs_is_admitted_and_one_without_purpose_is_refused() {
    let board = board();
    board
        .write(
            "prompts/full.md",
            &with_prompt(&json!({"inputs": {"repo": "The repository.", "pr_2": "The number."}})),
        )
        .write(
            "prompts/bare.md",
            &with_prompt(&json!({"runs_in": {"directory": "~/Code/example-org/home"}})),
        )
        .write(
            "prompts/purposeless.md",
            &with_prompt(&json!({"purpose": null})),
        );
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/bare.md", None),
            ("/prompts/full.md", None),
            ("/prompts/purposeless.md", Some("board::record::invalid")),
        ])
    );
}

#[test]
fn a_misspelt_field_is_refused() {
    let board = board();
    board
        .write(
            "prompts/spelt.md",
            &with_prompt(&json!({"inputs": {"pr": "The number."}})),
        )
        .write(
            "prompts/misspelt.md",
            &with_prompt(&json!({"input": {"pr": "The number."}})),
        );
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/misspelt.md", Some("board::record::invalid")),
            ("/prompts/spelt.md", None),
        ])
    );
}

#[test]
fn runs_in_holds_exactly_one_of_remote_and_directory() {
    let board = board();
    let cases = [
        ("remote", json!({"remote": "example-org/widgets"}), None),
        ("directory", json!({"directory": "/srv/{name}"}), None),
        (
            "both",
            json!({"remote": "example-org/widgets", "directory": "/srv"}),
            Some("board::record::invalid"),
        ),
        ("neither", json!({}), Some("board::record::invalid")),
        (
            "other",
            json!({"host": "x"}),
            Some("board::record::invalid"),
        ),
    ];
    for (name, runs_in, _) in &cases {
        let fields = json!({"runs_in": runs_in, "inputs": {"name": "A name."}});
        board.write(&format!("prompts/{name}.md"), &with_prompt(&fields));
    }
    let paths: Vec<String> = cases
        .iter()
        .map(|(name, _, _)| format!("/prompts/{name}.md"))
        .collect();
    let expected: Vec<(&str, Option<&str>)> = (paths.iter().zip(&cases))
        .map(|(path, (_, _, code))| (path.as_str(), *code))
        .collect();
    assert_eq!(board.checked("/prompts/"), codes(&expected));
}

#[test]
fn a_runs_in_naming_its_remote_repo_is_refused_and_its_twin_naming_it_remote_is_admitted() {
    let board = board();
    let inputs = json!({"repo": "The repository."});
    board
        .write(
            "prompts/remote.md",
            &with_prompt(&json!({"runs_in": {"remote": "{repo}"}, "inputs": inputs})),
        )
        .write(
            "prompts/repo.md",
            &with_prompt(&json!({"runs_in": {"repo": "{repo}"}, "inputs": inputs})),
        );
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/remote.md", None),
            ("/prompts/repo.md", Some("board::record::invalid")),
        ])
    );
}

#[test]
fn a_remote_is_owner_slash_name_or_an_input_and_a_directory_is_absolute_or_under_home() {
    let board = board();
    board
        .write(
            "prompts/owner.md",
            &with_prompt(&json!({"runs_in": {"remote": "example-org/{name}"}, "inputs": {"name": "A name."}})),
        )
        .write("prompts/bare-name.md", &with_prompt(&json!({"runs_in": {"remote": "widgets"}})))
        .write("prompts/home.md", &with_prompt(&json!({"runs_in": {"directory": "~/Code"}})))
        .write("prompts/relative.md", &with_prompt(&json!({"runs_in": {"directory": "Code/home"}})));
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/bare-name.md", Some("board::record::invalid")),
            ("/prompts/home.md", None),
            ("/prompts/owner.md", None),
            ("/prompts/relative.md", Some("board::record::invalid")),
        ])
    );
}

#[test]
fn an_input_name_is_lowercase_and_starts_with_a_letter() {
    let board = board();
    board
        .write(
            "prompts/lower.md",
            &with_prompt(&json!({"inputs": {"pr": "The number."}})),
        )
        .write(
            "prompts/upper.md",
            &with_prompt(&json!({"inputs": {"PR": "The number."}})),
        )
        .write(
            "prompts/digit.md",
            &with_prompt(&json!({"inputs": {"2pr": "The number."}})),
        );
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/digit.md", Some("board::record::invalid")),
            ("/prompts/lower.md", None),
            ("/prompts/upper.md", Some("board::record::invalid")),
        ])
    );
}

#[test]
fn a_directory_starting_with_an_input_is_the_input_alone_or_followed_by_a_slash() {
    let board = board();
    let at = |directory: &str| {
        with_prompt(
            &json!({"runs_in": {"directory": directory}, "inputs": {"base": "A directory."}}),
        )
    };
    board
        .write("prompts/alone.md", &at("{base}"))
        .write("prompts/under.md", &at("{base}/work"))
        .write("prompts/glued.md", &at("{base}work"));
    assert_eq!(
        board.checked("/prompts/"),
        codes(&[
            ("/prompts/alone.md", None),
            ("/prompts/glued.md", Some("board::record::invalid")),
            ("/prompts/under.md", None),
        ])
    );
}

/// `board check`'s code for a list of marked repos holding `at` under
/// `field`; none when it is valid.
fn marks_code(board: &Board, field: &str, at: &str) -> Option<String> {
    let marks = json!({"$type": "prompts/repos/v1", field: [at]});
    board.write("prompts/repos.json", &marks.to_string());
    board.checked("/prompts/")["/prompts/repos"].clone()
}

#[test]
fn the_list_of_marked_repos_references_mounts_and_refuses_a_path_that_is_not_a_reference() {
    let board = board();
    mounted(
        &board,
        "notes",
        &[("prompts/a.md", prompt("Notes", "", &json!({})))],
        false,
        true,
    );
    assert_eq!(marks_code(&board, "mounts", "/repos/notes"), None);
    assert_eq!(
        marks_code(&board, "mounts", "repos/notes").as_deref(),
        Some("board::ref::malformed")
    );
    assert_eq!(
        marks_code(&board, "repos", "/repos/notes").as_deref(),
        Some("board::record::invalid")
    );
}
