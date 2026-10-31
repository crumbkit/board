//! `put --new` with no folder: the folder the record's type declares in
//! `x-board-folder`, with each placeholder filled from `--var`.

use board_model::Code;
use board_scenario::{Item, Scenario};
use board_tree::LOCK_WAIT;

use super::{board, path};
use crate::put::Vars;
use crate::put::folder::filled;
use crate::tests::{open, task};

const QUESTION: &str = r#"{"$type": "lr/question/v1"}"#;

/// The test board, with `lr/question/v1` declaring a folder that
/// `declared` names.
fn questions(declared: &str) -> Scenario {
    let scenario = board();
    let schema = format!(r#"{{"type": "object", "x-board-folder": "{declared}"}}"#);
    scenario.write(
        ".board/types/lr/question/v1.schema.json",
        Item::File(&schema),
    );
    scenario
}

fn vars(pairs: &[(&str, &str)]) -> Vars {
    (pairs.iter())
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn a_put_with_no_folder_writes_in_the_folder_its_type_declares_with_each_var_filled_in() {
    let scenario = questions("/projects/{project}/questions/");
    let report = open(&scenario)
        .put_declared_waiting(
            &vars(&[("project", "board")]),
            QUESTION.as_bytes(),
            LOCK_WAIT,
        )
        .unwrap();
    let written = report.writes[0].display.to_string();
    assert!(
        written.starts_with("/projects/board/questions/2"),
        "{written}"
    );
    assert_eq!(report.writes[0].previous, None);
}

#[test]
fn placeholders_fill_from_vars_and_a_brace_without_its_close_is_kept() {
    let record_type = "lr/question/v1".parse().unwrap();
    let declared = "/{a}/x-{b}/{c/";
    let text = filled(&record_type, declared, &vars(&[("a", "1"), ("b", "2")])).unwrap();
    assert_eq!(text, "/1/x-2/{c/");
}

#[test]
fn an_unfilled_placeholder_is_refused_naming_it_and_its_twin_with_the_var_is_admitted() {
    let scenario = questions("/projects/{project}/questions/");
    let board = open(&scenario);
    let refused = board
        .declared_folder(&vars(&[]), QUESTION.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutFolderUnfilled);
    assert_eq!(
        refused.to_string(),
        "lr/question/v1 files records under /projects/{project}/questions/, and no --var gives {project}"
    );
    let fields = refused.fields();
    assert_eq!(fields.var.as_deref(), Some("project"));
    assert_eq!(fields.record_type, Some("lr/question/v1".parse().unwrap()));
    let admitted = board.declared_folder(&vars(&[("project", "board")]), QUESTION.as_bytes());
    assert_eq!(admitted.unwrap(), path("/projects/board/questions"));
}

#[test]
fn a_var_that_names_no_placeholder_is_refused_and_its_twin_without_it_is_admitted() {
    let scenario = questions("/projects/{project}/questions/");
    let board = open(&scenario);
    let extra = vars(&[("project", "board"), ("team", "a")]);
    let refused = board
        .declared_folder(&extra, QUESTION.as_bytes())
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutVarUnused);
    assert_eq!(
        refused.to_string(),
        "--var team names no placeholder of the folder /projects/{project}/questions/"
    );
    assert_eq!(refused.fields().var.as_deref(), Some("team"));
    let only = vars(&[("project", "board")]);
    assert!(board.declared_folder(&only, QUESTION.as_bytes()).is_ok());
}

#[test]
fn a_type_that_declares_no_folder_is_refused_naming_it_and_a_type_that_does_is_admitted() {
    let scenario = questions("/questions/");
    let board = open(&scenario);
    let refused = (board.declared_folder(&vars(&[]), task("").as_bytes())).unwrap_err();
    assert_eq!(refused.code(), Code::PutNoFolder);
    assert_eq!(
        refused.to_string(),
        "lr/task/v1 declares no folder, so `put --new` needs one"
    );
    assert_eq!(
        refused.fields().record_type,
        Some("lr/task/v1".parse().unwrap())
    );
    let mount = r#"{"$type": "board/mount/v1", "path": "/projects/p"}"#;
    let refused = (board.declared_folder(&vars(&[]), mount.as_bytes())).unwrap_err();
    assert_eq!(refused.code(), Code::PutNoFolder);
    assert_eq!(
        refused.fields().record_type,
        Some("board/mount/v1".parse().unwrap())
    );
    let admitted = board.declared_folder(&vars(&[]), QUESTION.as_bytes());
    assert_eq!(admitted.unwrap(), path("/questions"));
}

#[test]
fn a_record_refused_before_its_folder_is_found_keeps_its_code_and_a_valid_twin_is_admitted() {
    let scenario = questions("/questions/");
    let board = open(&scenario);
    let refused = (board.declared_folder(&vars(&[]), b"{}")).unwrap_err();
    assert_eq!(refused.code(), Code::PutInvalid);
    assert_eq!(
        refused.to_string(),
        "the record is refused: it has no $type"
    );
    assert!(
        board
            .declared_folder(&vars(&[]), QUESTION.as_bytes())
            .is_ok()
    );
}

#[test]
fn a_filled_folder_that_is_not_a_logical_path_is_refused_and_a_well_formed_one_is_admitted() {
    let scenario = questions("/projects/{project}/");
    let board = open(&scenario);
    let dots =
        (board.declared_folder(&vars(&[("project", "..")]), QUESTION.as_bytes())).unwrap_err();
    assert_eq!(dots.code(), Code::PathMalformed);
    assert!((board.declared_folder(&vars(&[("project", "p")]), QUESTION.as_bytes())).is_ok());
}
