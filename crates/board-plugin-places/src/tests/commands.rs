//! `attach` and `detach` over a real board: what each writes or removes,
//! and each refusal beside a twin that differs in one value.

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::host::{Answer, LIVE, MACHINE, Real, id, row};
use super::join;
use crate::attach::attach;
use crate::detach::detach;

const JOIN: &str = "/places/quokka/fix-login/changes/fix-login";

/// A host with the change `/changes/fix-login` and a live workspace
/// `fix-login` with id `a`.
fn ready() -> Real {
    let host = Real::new();
    host.change("/changes/fix-login", "build");
    host.mxv(&[row("fix-login", "/w/fix-login", Some(&id('a')))], &[]);
    host
}

fn code(result: Result<Value, board_ops::PluginError>) -> String {
    result.unwrap_err().code
}

#[test]
fn attach_writes_the_join_with_the_workspaces_id_and_who_attached_it() {
    let host = ready();
    let output = attach(
        &host,
        &json!({"change": "/changes/fix-login", "workspace": "fix-login", "program": "claude-code", "agent": " "}),
    )
    .unwrap();
    assert_eq!(
        output,
        json!({
            "path": JOIN,
            "machine": MACHINE,
            "workspace": "fix-login",
            "change": "/changes/fix-login",
            "workspace_id": id('a'),
            "written": true,
            "replaced": false,
        })
    );
    assert_eq!(
        host.record(JOIN).unwrap(),
        join(&json!({"workspace_id": id('a'), "by": {"program": "claude-code"}}))
    );
    assert_eq!(host.puts.borrow()[0]["expect"], "none");
    assert_eq!(*host.runs.borrow(), [LIVE]);
}

#[test]
fn attach_again_with_the_same_id_writes_nothing_and_with_a_new_id_replaces_the_join() {
    let host = ready();
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    attach(&host, &input).unwrap();
    let again = attach(&host, &input).unwrap();
    assert_eq!(
        (&again["written"], &again["replaced"]),
        (&json!(false), &json!(false))
    );
    assert_eq!(host.puts.borrow().len(), 1);

    host.mxv(&[row("fix-login", "/w/fix-login", Some(&id('b')))], &[]);
    let replaced = attach(&host, &input).unwrap();
    assert_eq!(
        (&replaced["written"], &replaced["replaced"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(host.record(JOIN).unwrap()["workspace_id"], id('b'));
    let puts = host.puts.borrow();
    assert!(
        puts[1]["expect"].as_str().unwrap().len() >= 40,
        "{}",
        puts[1]
    );
}

#[test]
fn attach_without_a_workspace_uses_the_one_that_holds_the_caller_and_outside_any_is_refused() {
    let tree = Scenario::new();
    tree.write("ws/fix-login/board/.keep", Item::File(""))
        .write("elsewhere/.keep", Item::File(""));
    let root = tree
        .workdir()
        .join("ws/fix-login")
        .to_str()
        .unwrap()
        .to_owned();
    let input = json!({"change": "/changes/fix-login"});
    let host = Real::new().caller(tree.workdir().join("elsewhere"));
    host.change("/changes/fix-login", "build");
    host.mxv(&[row("fix-login", &root, Some(&id('a')))], &[]);
    let refused = attach(&host, &input).unwrap_err();
    assert_eq!(refused.code, "places::workspace::outside");
    assert!(
        refused.message.contains("--workspace"),
        "{}",
        refused.message
    );

    let host = Real::new().caller(tree.workdir().join("ws/fix-login/board"));
    host.change("/changes/fix-login", "build");
    host.mxv(&[row("fix-login", &root, Some(&id('a')))], &[]);
    assert_eq!(attach(&host, &input).unwrap()["path"], JOIN);
}

#[test]
fn attach_with_no_caller_and_no_workspace_is_outside_and_with_a_workspace_is_admitted() {
    let host = ready();
    assert_eq!(
        code(attach(&host, &json!({"change": "/changes/fix-login"}))),
        "places::workspace::outside"
    );
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_to_a_workspace_mxv_does_not_list_is_refused_and_to_one_it_lists_is_admitted() {
    let host = ready();
    let refused = attach(
        &host,
        &json!({"change": "/changes/fix-login", "workspace": "fix-logon"}),
    )
    .unwrap_err();
    assert_eq!(refused.code, "places::workspace::not_found");
    assert_eq!(refused.fields["workspace"], "fix-logon");
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_to_a_workspace_with_a_null_id_is_refused_naming_why_and_one_with_an_id_is_admitted() {
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    let host = ready();
    host.mxv(&[row("fix-login", "/w/fix-login", None)], &[]);
    let refused = attach(&host, &input).unwrap_err();
    assert_eq!(refused.code, "places::workspace::no_id");
    assert!(
        refused.message.contains("predates ids"),
        "{}",
        refused.message
    );

    let mut broken = row("fix-login", "/w/fix-login", None);
    broken["problem"] = json!("the record does not parse");
    host.mxv(&[broken], &[]);
    let refused = attach(&host, &input).unwrap_err();
    assert_eq!(refused.code, "places::workspace::no_id");
    assert!(
        refused.message.contains("the record does not parse"),
        "{}",
        refused.message
    );
    assert_eq!(host.puts.borrow().len(), 0);

    host.mxv(&[row("fix-login", "/w/fix-login", Some(&id('a')))], &[]);
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_with_an_mxv_too_old_to_print_ids_is_refused_and_with_a_current_one_is_admitted() {
    let host = ready();
    let old = r#"{"schema":"mxv/failure/v1","code":"mxv::cli::usage","message":"--fields names an unknown field 'id'","fields":{"argument":"--fields","value":"id"}}"#;
    host.answer(
        LIVE,
        Answer::Exits {
            code: 2,
            stdout: old.to_owned(),
            stderr: old.to_owned(),
        },
    );
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    let refused = attach(&host, &input).unwrap_err();
    assert_eq!(refused.code, "places::mxv::no_id");
    assert!(
        refused.message.contains("upgrade mxv to 0.4.0"),
        "{}",
        refused.message
    );
    host.mxv(&[row("fix-login", "/w/fix-login", Some(&id('a')))], &[]);
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_without_mxv_is_failed_and_with_it_is_admitted() {
    let host = Real::new();
    host.change("/changes/fix-login", "build");
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(code(attach(&host, &input)), "places::mxv::failed");
    host.mxv(&[row("fix-login", "/w/fix-login", Some(&id('a')))], &[]);
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_to_a_path_with_nothing_or_no_change_there_is_refused_and_to_a_change_is_admitted() {
    let host = ready();
    host.write("/tasks/fix-login", &json!({"$type": "board/config/v1"}));
    host.board
        .write("notes/fix-login/a.md", Item::File("# A\n"));
    for (change, refused) in [
        ("/changes/fix-logon", "places::change::not_found"),
        ("/tasks/fix-login", "places::change::wrong_type"),
        ("/notes/fix-login", "places::change::wrong_type"),
    ] {
        let input = json!({"change": change, "workspace": "fix-login"});
        let error = attach(&host, &input).unwrap_err();
        assert_eq!(error.code, refused, "{change}: {}", error.message);
        assert_eq!(error.fields["change"], change);
    }
    assert_eq!(host.puts.borrow().len(), 0);
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_where_a_join_to_another_change_of_the_same_name_is_held_is_taken_and_to_the_same_change_is_not()
 {
    let host = ready();
    host.change("/projects/other/changes/fix-login", "build");
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    attach(&host, &input).unwrap();
    let other = json!({"change": "/projects/other/changes/fix-login", "workspace": "fix-login"});
    let refused = attach(&host, &other).unwrap_err();
    assert_eq!(refused.code, "places::attach::taken");
    assert_eq!(refused.fields["change"], "/changes/fix-login");
    assert_eq!(attach(&host, &input).unwrap()["written"], false);
}

#[test]
fn attach_over_a_held_join_this_plugin_cannot_read_is_refused_and_over_a_whole_one_is_not() {
    let host = ready();
    host.write(
        JOIN,
        &json!({"$type": "places/workspace-change/v1", "change": "/changes/fix-login"}),
    );
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    let refused = attach(&host, &input).unwrap_err();
    assert_eq!(refused.code, "places::record::unreadable");
    assert_eq!(refused.fields["path"], JOIN);
    host.write(JOIN, &join(&json!({"workspace_id": id('a')})));
    assert_eq!(attach(&host, &input).unwrap()["written"], false);
}

#[test]
fn attach_on_an_unnamed_machine_is_refused_before_mxv_runs_and_on_a_named_one_is_admitted() {
    let host = ready().environment(&[]);
    host.change("/changes/fix-login", "build");
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(code(attach(&host, &input)), "places::machine::unnamed");
    assert!(!host.runs.borrow().iter().any(|run| run.starts_with("mxv")));
    let host = ready().environment(&[("BOARD_MACHINE", "bilby")]);
    assert_eq!(
        attach(&host, &input).unwrap()["path"],
        "/places/bilby/fix-login/changes/fix-login"
    );
}

#[test]
fn attach_to_a_workspace_whose_name_is_no_path_segment_is_refused_with_boards_code_and_its_twin_is_not()
 {
    let host = ready();
    let long = "w".repeat(256);
    host.mxv(
        &[
            row(&long, "/w/long", Some(&id('a'))),
            row("fix-login", "/w/fix-login", Some(&id('a'))),
        ],
        &[],
    );
    let refused = attach(
        &host,
        &json!({"change": "/changes/fix-login", "workspace": long}),
    )
    .unwrap_err();
    assert_eq!(refused.code, "places::board::refused");
    assert_eq!(refused.fields["code"], "board::path::malformed");
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(attach(&host, &input).unwrap()["written"], true);
}

#[test]
fn attach_writes_since_as_the_time_the_call_started_to_the_second() {
    let now = board_model::moment("2026-11-21T08:30:15.75Z").unwrap();
    let host = ready().at(now);
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    attach(&host, &input).unwrap();
    assert_eq!(host.record(JOIN).unwrap()["since"], "2026-11-21T08:30:15Z");
}

#[test]
fn detach_removes_the_join_and_reports_false_when_none_is_there() {
    let host = ready();
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    attach(&host, &input).unwrap();
    let removed = detach(&host, &input).unwrap();
    assert_eq!(removed, json!({"path": JOIN, "removed": true}));
    assert_eq!(host.record(JOIN), None);
    let line = &host.puts.borrow()[1];
    assert_eq!(
        (&line["remove"], &line["ignore_unmatch"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(
        detach(&host, &input).unwrap(),
        json!({"path": JOIN, "removed": false})
    );
}

#[test]
fn detach_with_a_workspace_runs_no_mxv_and_without_one_asks_mxv_which_holds_the_caller() {
    let host = Real::new().environment(&[("BOARD_MACHINE", MACHINE)]);
    host.write(JOIN, &join(&json!({})));
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(detach(&host, &input).unwrap()["removed"], true);
    assert_eq!(host.runs.borrow().len(), 0);

    let tree = Scenario::new();
    tree.write("ws/fix-login/board/.keep", Item::File(""));
    let root = tree
        .workdir()
        .join("ws/fix-login")
        .to_str()
        .unwrap()
        .to_owned();
    let host = Real::new().caller(tree.workdir().join("ws/fix-login/board"));
    host.write(JOIN, &join(&json!({})));
    host.mxv(&[row("fix-login", &root, None)], &[]);
    let removed = detach(&host, &json!({"change": "/changes/fix-login"})).unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(*host.runs.borrow(), [LIVE]);
}

#[test]
fn detach_leaves_a_join_to_another_change_of_the_same_name_and_removes_its_own() {
    let host = ready();
    host.write(
        JOIN,
        &join(&json!({"change": "/projects/other/changes/fix-login"})),
    );
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(detach(&host, &input).unwrap()["removed"], false);
    assert!(host.record(JOIN).is_some());
    {
        let warnings = host.warnings.borrow();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("places::detach::other_change")
                && warnings[0].contains("/projects/other/changes/fix-login"),
            "{warnings:?}"
        );
    }
    let other = json!({"change": "/projects/other/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(detach(&host, &other).unwrap()["removed"], true);
    assert_eq!(host.warnings.borrow().len(), 1);
}

#[test]
fn detach_over_a_held_join_this_plugin_cannot_read_is_refused_and_over_a_whole_one_is_not() {
    let host = ready();
    host.write(JOIN, &json!({"$type": "places/workspace-change/v1"}));
    let input = json!({"change": "/changes/fix-login", "workspace": "fix-login"});
    assert_eq!(code(detach(&host, &input)), "places::record::unreadable");
    host.write(JOIN, &join(&json!({})));
    assert_eq!(detach(&host, &input).unwrap()["removed"], true);
}
