//! `list`: the status each join's workspace has, and the rows it prints,
//! with null title and phase for a change it cannot read.

use serde_json::{Value, json};

use super::host::{ARCHIVED, Answer, LIVE, MACHINE, Real, id, row};
use super::join;
use crate::list::{Status, list, status};
use crate::mxv::Workspace;

fn workspace(id: Option<&str>) -> Workspace {
    Workspace {
        name: "fix-login".to_owned(),
        path: "/w/fix-login".to_owned(),
        id: id.map(str::to_owned),
        problem: None,
    }
}

#[test]
fn a_join_has_each_of_the_five_statuses_by_what_mxv_lists_under_its_name() {
    let (a, b) = (id('a'), id('b'));
    let live_a = workspace(Some(&a));
    let live_b = workspace(Some(&b));
    let null = workspace(None);
    assert_eq!(status(&a, Some(&live_a), &[]), Status::Live);
    assert_eq!(status(&a, Some(&live_b), &[&live_a]), Status::Replaced);
    assert_eq!(status(&a, Some(&null), &[]), Status::Unknown);
    assert_eq!(status(&a, None, &[&live_b, &live_a]), Status::Archived);
    assert_eq!(status(&a, None, &[&live_b]), Status::Unknown);
    assert_eq!(status(&a, None, &[&null]), Status::Unknown);
    assert_eq!(status(&a, None, &[]), Status::Gone);
}

#[test]
fn statuses_print_as_lowercase_words() {
    let printed: Vec<Value> = [
        Status::Live,
        Status::Replaced,
        Status::Archived,
        Status::Gone,
        Status::Unknown,
    ]
    .iter()
    .map(|status| serde_json::to_value(status).unwrap())
    .collect();
    assert_eq!(
        printed,
        [
            json!("live"),
            json!("replaced"),
            json!("archived"),
            json!("gone"),
            json!("unknown")
        ]
    );
}

/// A host holding a join for each of `workspaces` to `/changes/<workspace>`
/// on `quokka` with id `a`, and one on another machine.
fn joined(workspaces: &[&str]) -> Real {
    let host = Real::new();
    for workspace in workspaces {
        host.write(
            &format!("/places/{MACHINE}/{workspace}/changes/{workspace}"),
            &join(&json!({
                "workspace": format!("/places/{MACHINE}/{workspace}"),
                "change": format!("/changes/{workspace}"),
                "workspace_id": id('a'),
            })),
        );
    }
    host.write(
        "/places/wombat/elsewhere/changes/elsewhere",
        &join(&json!({"workspace": "/places/wombat/elsewhere", "change": "/changes/elsewhere"})),
    );
    host
}

#[test]
fn list_prints_every_machines_joins_with_their_changes_and_statuses() {
    let host = joined(&["archived", "gone", "live", "null", "replaced"]);
    for name in ["archived", "live", "null", "replaced"] {
        host.change(&format!("/changes/{name}"), "build");
    }
    let (a, b) = (id('a'), id('b'));
    host.mxv(
        &[
            row("live", "/w/live", Some(&a)),
            row("replaced", "/w/replaced", Some(&b)),
            row("null", "/w/null", None),
        ],
        &[row("archived", "/w/archived", Some(&a))],
    );
    let output = list(&host, &json!({})).unwrap();
    assert_eq!(
        (&output["machine"], &output["total"], &output["truncated"]),
        (&json!(MACHINE), &json!(6), &json!(false))
    );
    let statuses: Vec<(&str, &str, &str)> = (output["joins"].as_array().unwrap().iter())
        .map(|row| {
            (
                row["machine"].as_str().unwrap(),
                row["workspace"].as_str().unwrap(),
                row["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        [
            ("quokka", "archived", "archived"),
            ("quokka", "gone", "gone"),
            ("quokka", "live", "live"),
            ("quokka", "null", "unknown"),
            ("quokka", "replaced", "replaced"),
            ("wombat", "elsewhere", "unknown"),
        ]
    );
    assert_eq!(
        output["joins"][2],
        json!({
            "path": "/places/quokka/live/changes/live",
            "machine": "quokka",
            "workspace": "live",
            "workspace_id": a,
            "since": "2026-11-20T09:00:00Z",
            "change": {"path": "/changes/live", "title": "Work on /changes/live", "phase": "build"},
            "status": "live",
        })
    );
    assert_eq!(*host.runs.borrow(), [LIVE, ARCHIVED]);
}

#[test]
fn a_change_that_cannot_be_read_keeps_its_row_with_null_title_and_phase_and_one_warning() {
    let host = joined(&["gone", "live"]);
    host.change("/changes/live", "review");
    host.mxv(&[row("live", "/w/live", Some(&id('a')))], &[]);
    let output = list(&host, &json!({"machine": MACHINE})).unwrap();
    assert_eq!(
        output["joins"][0]["change"],
        json!({"path": "/changes/gone", "title": null, "phase": null})
    );
    assert_eq!(output["joins"][1]["change"]["phase"], "review");
    let warnings = host.warnings.borrow();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("board::path::not_found"),
        "{warnings:?}"
    );
}

#[test]
fn a_change_path_holding_another_record_type_keeps_its_row_with_one_wrong_type_warning() {
    let host = joined(&["live", "odd"]);
    host.change("/changes/live", "review");
    host.write("/changes/odd", &json!({"$type": "board/config/v1"}));
    host.mxv(&[], &[]);
    let output = list(&host, &json!({"machine": MACHINE})).unwrap();
    assert_eq!(output["joins"][0]["change"]["phase"], "review");
    assert_eq!(
        output["joins"][1]["change"],
        json!({"path": "/changes/odd", "title": null, "phase": null})
    );
    let warnings = host.warnings.borrow();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("places::change::wrong_type"),
        "{warnings:?}"
    );
}

#[test]
fn the_limit_leaves_joins_out_and_says_so_and_all_returns_every_one() {
    let host = joined(&["a", "b", "c"]);
    host.mxv(&[], &[]);
    let limited = list(&host, &json!({"limit": 2, "machine": MACHINE})).unwrap();
    assert_eq!(
        (&limited["total"], &limited["truncated"]),
        (&json!(3), &json!(true))
    );
    assert_eq!(limited["joins"].as_array().unwrap().len(), 2);
    let every = list(&host, &json!({"limit": 2, "all": true, "machine": MACHINE})).unwrap();
    assert_eq!(
        (&every["total"], &every["truncated"]),
        (&json!(3), &json!(false))
    );
    assert_eq!(every["joins"].as_array().unwrap().len(), 3);
}

#[test]
fn a_join_this_plugin_cannot_read_is_left_out_with_a_warning_and_a_whole_one_is_listed() {
    let host = joined(&["whole"]);
    host.write(
        "/places/quokka/broken/changes/broken",
        &join(&json!({"workspace_id": "short"})),
    );
    host.mxv(&[], &[]);
    let output = list(&host, &json!({"machine": MACHINE})).unwrap();
    assert_eq!(output["total"], 1);
    assert_eq!(output["joins"][0]["workspace"], "whole");
    let warnings = host.warnings.borrow();
    assert!(
        warnings.iter().any(|line| line
            .starts_with("places::record::unreadable: /places/quokka/broken/changes/broken")),
        "{warnings:?}"
    );
}

#[test]
fn list_with_an_mxv_too_old_to_print_ids_is_refused_and_with_a_current_one_is_answered() {
    let host = joined(&["live"]);
    let old = r#"{"schema":"mxv/failure/v1","code":"mxv::cli::usage","message":"--fields names an unknown field 'id'","fields":{"argument":"--fields","value":"id"}}"#;
    host.answer(
        LIVE,
        Answer::Exits {
            code: 2,
            stdout: old.to_owned(),
            stderr: old.to_owned(),
        },
    );
    assert_eq!(
        list(&host, &json!({})).unwrap_err().code,
        "places::mxv::no_id"
    );
    host.mxv(&[row("live", "/w/live", Some(&id('a')))], &[]);
    assert_eq!(
        list(&host, &json!({})).unwrap()["joins"][0]["status"],
        "live"
    );
}

#[test]
fn list_with_a_listing_cut_short_is_refused_and_with_a_whole_one_is_answered() {
    let host = joined(&["live"]);
    host.mxv(&[], &[]);
    let cut = json!({"schema": "mxv/list/v1", "data": [], "total": 30, "truncated": true});
    host.answer(ARCHIVED, Answer::printing(&cut.to_string()));
    assert_eq!(
        list(&host, &json!({})).unwrap_err().code,
        "places::mxv::cut"
    );
    host.mxv(&[], &[]);
    assert_eq!(
        list(&host, &json!({})).unwrap()["joins"][0]["status"],
        "gone"
    );
}

#[test]
fn list_on_an_unnamed_machine_is_refused_before_it_reads_and_on_a_named_one_is_answered() {
    let host = joined(&["live"]).environment(&[]);
    host.mxv(&[], &[]);
    assert_eq!(
        list(&host, &json!({})).unwrap_err().code,
        "places::machine::unnamed"
    );
    assert!(!host.runs.borrow().iter().any(|run| run.starts_with("mxv")));
    let host = joined(&["live"]);
    host.mxv(&[], &[]);
    let output: Value = list(&host, &json!({})).unwrap();
    assert_eq!(output["machine"], MACHINE);
}

#[test]
fn a_join_of_another_machine_is_unknown_and_listing_only_it_asks_mxv_nothing() {
    let host = joined(&["live"]);
    let output = list(&host, &json!({"machine": "wombat"})).unwrap();
    assert_eq!(
        (&output["machine"], &output["total"]),
        (&json!(MACHINE), &json!(1))
    );
    assert_eq!(
        (
            &output["joins"][0]["machine"],
            &output["joins"][0]["status"]
        ),
        (&json!("wombat"), &json!("unknown"))
    );
    assert_eq!(host.runs.borrow().len(), 0);
    host.mxv(&[row("live", "/w/live", Some(&id('a')))], &[]);
    let mine = list(&host, &json!({"machine": MACHINE})).unwrap();
    assert_eq!(mine["joins"][0]["status"], "live");
    assert_eq!(*host.runs.borrow(), [LIVE, ARCHIVED]);
}

#[test]
fn a_machine_with_no_joins_lists_none_and_a_machine_with_joins_lists_its_own() {
    let host = joined(&["live"]);
    let none = list(&host, &json!({"machine": "bilby"})).unwrap();
    assert_eq!(
        (&none["joins"], &none["total"], &none["truncated"]),
        (&json!([]), &json!(0), &json!(false))
    );
    let wombat = list(&host, &json!({"machine": "wombat"})).unwrap();
    assert_eq!(wombat["joins"][0]["workspace"], "elsewhere");
}
