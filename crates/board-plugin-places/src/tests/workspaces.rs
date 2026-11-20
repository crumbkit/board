//! What mxv's runs become: rows with an id string or a null id, an mxv too
//! old to print ids, a listing cut short, and mxv failing; and which
//! workspace holds a directory.

use std::path::Path;

use board_plugin::child::{Exited, ProcessError};
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::host::{id, listing, row};
use crate::mxv::{Listing, Workspace, arguments, holding, rows};

fn exited(code: i32, stdout: &str, stderr: &str) -> Exited {
    Exited {
        code: Some(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn code(ran: &Result<Exited, ProcessError>) -> String {
    rows(ran).unwrap_err().code
}

fn workspace(name: &str, path: &str, id: Option<&str>) -> Workspace {
    Workspace {
        name: name.to_owned(),
        path: path.to_owned(),
        id: id.map(str::to_owned),
        problem: None,
    }
}

#[test]
fn the_listings_ask_for_every_workspace_with_its_name_id_and_path() {
    assert_eq!(
        arguments(Listing::Live).join(" "),
        "list --all --json --fields name,id,path"
    );
    assert_eq!(
        arguments(Listing::Archived).join(" "),
        "list --archived --all --json --fields name,id,path"
    );
}

#[test]
fn a_row_with_an_id_string_keeps_it_and_one_with_null_or_a_malformed_id_has_none() {
    let a = id('a');
    let printed = listing(&[
        row("one", "/w/one", Some(&a)),
        row("two", "/w/two", None),
        row("three", "/w/three", Some("ABC")),
    ]);
    assert_eq!(
        rows(&Ok(exited(0, &printed, ""))).unwrap(),
        [
            workspace("one", "/w/one", Some(&a)),
            workspace("two", "/w/two", None),
            workspace("three", "/w/three", None),
        ]
    );
}

#[test]
fn a_row_keeps_mxvs_problem_when_it_has_one_and_not_when_it_is_empty() {
    let mut broken = row("one", "/w/one", None);
    broken["problem"] = json!("the record does not parse");
    let mut whole = row("two", "/w/two", None);
    whole["problem"] = json!("");
    let read = rows(&Ok(exited(0, &listing(&[broken, whole]), ""))).unwrap();
    assert_eq!(
        read[0].problem.as_deref(),
        Some("the record does not parse")
    );
    assert_eq!(read[1].problem, None);
}

#[test]
fn a_row_with_no_id_key_is_no_id_and_its_twin_with_a_null_id_is_read() {
    let mut old = row("one", "/w/one", None);
    old.as_object_mut().unwrap().remove("id");
    let refused = rows(&Ok(exited(0, &listing(&[old]), ""))).unwrap_err();
    assert_eq!(refused.code, "places::mxv::no_id");
    assert!(refused.message.contains("0.3.0"), "{}", refused.message);
    assert_eq!(
        rows(&Ok(exited(0, &listing(&[row("one", "/w/one", None)]), "")))
            .unwrap()
            .len(),
        1
    );
}

/// mxv 0.1.1's refusal of `--fields name,id,path`, which it prints on
/// stdout and stderr.
const OLD: &str = r#"{"schema":"mxv/failure/v1","code":"mxv::cli::usage","message":"--fields names an unknown field 'id'","fields":{"argument":"--fields","value":"id"}}"#;

#[test]
fn a_usage_refusal_naming_id_is_no_id_and_one_naming_another_field_is_failed() {
    assert_eq!(code(&Ok(exited(2, OLD, OLD))), "places::mxv::no_id");
    assert_eq!(code(&Ok(exited(2, "", OLD))), "places::mxv::no_id");
    let other = OLD
        .replace("'id'", "'ids'")
        .replace(r#""value":"id""#, r#""value":"ids""#);
    let refused = rows(&Ok(exited(2, &other, &other))).unwrap_err();
    assert_eq!(refused.code, "places::mxv::failed");
    assert_eq!(
        refused.message,
        "mxv exited 2: --fields names an unknown field 'ids'"
    );
}

#[test]
fn a_listing_cut_short_is_refused_and_its_whole_twin_is_read() {
    let mut cut: Value = serde_json::from_str(&listing(&[row("one", "/w/one", None)])).unwrap();
    cut["truncated"] = json!(true);
    assert_eq!(
        code(&Ok(exited(0, &cut.to_string(), ""))),
        "places::mxv::cut"
    );
    cut["truncated"] = json!(false);
    assert_eq!(rows(&Ok(exited(0, &cut.to_string(), ""))).unwrap().len(), 1);
}

#[test]
fn mxv_missing_stopped_failing_or_printing_no_listing_is_failed_with_why() {
    let failed = |ran: Result<Exited, ProcessError>| {
        let refused = rows(&ran).unwrap_err();
        assert_eq!(refused.code, "places::mxv::failed");
        refused.message
    };
    let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
    assert_eq!(
        failed(Err(ProcessError::NotStarted { source: missing })),
        "mxv is not on PATH"
    );
    assert_eq!(
        failed(Err(ProcessError::Timeout { stderr: Vec::new() })),
        "mxv ran past 10 s"
    );
    assert_eq!(
        failed(Err(ProcessError::TooLarge {
            limit: 16,
            stderr: Vec::new()
        })),
        "mxv printed more than 16 bytes"
    );
    assert_eq!(
        failed(Ok(exited(1, "", "starting\nno store\n"))),
        "mxv exited 1: no store"
    );
    assert_eq!(
        failed(Ok(exited(0, "not json", ""))),
        "mxv printed no mxv/list/v1 document"
    );
    assert_eq!(
        failed(Ok(exited(0, r#"{"schema":"mxv/show/v1","data":[]}"#, ""))),
        "mxv printed no mxv/list/v1 document"
    );
    assert_eq!(rows(&Ok(exited(0, &listing(&[]), ""))).unwrap(), []);
}

/// A tree of directories: `ws/outer`, `ws/outer/inner`, `ws/sibling`, and
/// `link`, a symbolic link to `ws/outer`.
fn tree() -> Scenario {
    let tree = Scenario::new();
    tree.write("ws/outer/inner/board/.keep", Item::File(""))
        .write("ws/sibling/board/.keep", Item::File(""))
        .write("link", Item::Symlink("ws/outer"));
    tree
}

fn held(workspaces: &[Workspace], directory: &Path) -> Option<String> {
    holding(workspaces, directory).map(|workspace| workspace.name.clone())
}

#[test]
fn the_innermost_workspace_whose_directory_holds_the_caller_holds_it_and_a_sibling_does_not() {
    let tree = tree();
    let at = |path: &str| tree.workdir().join(path).to_str().unwrap().to_owned();
    let workspaces = [
        workspace("outer", &at("ws/outer"), None),
        workspace("inner", &at("ws/outer/inner"), None),
        workspace("sibling", &at("ws/sibling"), None),
        workspace("removed", &at("ws/removed"), None),
    ];
    let inside = tree.workdir().join("ws/outer/inner/board");
    assert_eq!(held(&workspaces, &inside).as_deref(), Some("inner"));
    assert_eq!(
        held(&workspaces, &tree.workdir().join("ws/outer")).as_deref(),
        Some("outer")
    );
    assert_eq!(
        held(&workspaces, &tree.workdir().join("ws/sibling/board")).as_deref(),
        Some("sibling")
    );
    assert_eq!(held(&workspaces, &tree.workdir().join("ws")), None);
    assert_eq!(held(&workspaces, &tree.workdir().join("ws/removed")), None);
}

#[test]
fn a_caller_or_workspace_reached_through_a_symbolic_link_is_held_as_its_target_is() {
    let tree = tree();
    let at = |path: &str| tree.workdir().join(path).to_str().unwrap().to_owned();
    let through_link = [workspace("outer", &at("link"), None)];
    assert_eq!(
        held(&through_link, &tree.workdir().join("ws/outer/inner")).as_deref(),
        Some("outer")
    );
    let direct = [workspace("outer", &at("ws/outer"), None)];
    assert_eq!(
        held(&direct, &tree.workdir().join("link/inner")).as_deref(),
        Some("outer")
    );
    assert_eq!(held(&direct, &tree.workdir().join("ws/sibling")), None);
}
