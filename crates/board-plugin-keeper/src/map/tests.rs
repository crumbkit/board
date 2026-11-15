//! Each refusal of `check` has an admitted twin that differs only in the
//! thing refused.

use serde_json::json;

use super::*;

fn known() -> Known {
    let set = |paths: &[&str]| paths.iter().map(|path| (*path).to_owned()).collect();
    Known {
        open: set(&["/changes/a", "/changes/b"]),
        changes: set(&["/changes/a", "/changes/b", "/changes/old"]),
    }
}

fn input(edit: impl FnOnce(&mut Value)) -> Input {
    let mut value = json!({
        "source": "/curation.md",
        "realms": [{"id": "alpha", "title": "Alpha"}, {"id": "beta", "title": "Beta"}],
        "projects": [
            {"id": "speed", "realm": "alpha", "title": "Speed", "state": "active",
             "task": null, "changes": ["/changes/a", "/changes/b"],
             "order": [["/changes/a", "/changes/b"]]},
            {"id": "docs", "realm": "beta", "title": "Docs", "state": "queued",
             "task": "/tasks/docs", "changes": []},
        ],
        "findings": [{"kind": "landed", "paths": ["/changes/old"], "evidence": "Merged.",
                      "fix": "changes.close"}],
        "links": [{"before": "/changes/a", "after": "/changes/b", "project": "speed"}],
        "systems": [{"id": "api", "realm": "alpha", "title": "API"}],
        "question_rules": {
            "mount_realms": {"team": "beta"}, "localRealm": {"notes": "alpha"},
            "localDefault": "beta", "tasks": [{"contains": "x", "to": "speed"}],
            "names": {"guide": "docs"},
        },
    });
    edit(&mut value);
    serde_json::from_value(value).expect("the sample input has the input's shape")
}

fn code(input: &Input) -> Option<String> {
    check(input, &known()).err().map(|error| error.code)
}

#[test]
fn a_whole_tree_is_admitted() {
    assert_eq!(code(&input(|_| {})), None);
}

#[test]
fn an_open_change_in_no_project_is_refused_and_placing_it_admits_the_map() {
    let left_out = input(|value| {
        value["projects"][0]["changes"] = json!(["/changes/a"]);
        value["projects"][0]["order"] = json!([]);
        value["links"] = json!([]);
    });
    assert_eq!(code(&left_out).as_deref(), Some("keeper::map::unplaced"));
    let placed = input(|value| {
        value["projects"][0]["changes"] = json!(["/changes/a"]);
        value["projects"][0]["order"] = json!([]);
        value["projects"][1]["changes"] = json!(["/changes/b"]);
        value["links"] = json!([]);
    });
    assert_eq!(code(&placed), None);
}

#[test]
fn a_change_in_two_projects_is_refused() {
    let twice = input(|value| value["projects"][1]["changes"] = json!(["/changes/a"]));
    assert_eq!(code(&twice).as_deref(), Some("keeper::map::duplicate"));
}

#[test]
fn a_realm_or_project_listed_twice_is_refused() {
    let realms = input(|value| value["realms"][1]["id"] = json!("alpha"));
    assert_eq!(code(&realms).as_deref(), Some("keeper::map::duplicate_id"));
    let projects = input(|value| value["projects"][1]["id"] = json!("speed"));
    assert_eq!(
        code(&projects).as_deref(),
        Some("keeper::map::duplicate_id")
    );
}

#[test]
fn a_project_or_system_in_an_unlisted_realm_is_refused() {
    let project = input(|value| value["projects"][1]["realm"] = json!("gamma"));
    assert_eq!(
        code(&project).as_deref(),
        Some("keeper::map::unknown_realm")
    );
    let system = input(|value| value["systems"][0]["realm"] = json!("gamma"));
    assert_eq!(code(&system).as_deref(), Some("keeper::map::unknown_realm"));
}

#[test]
fn a_path_that_is_no_change_is_refused_where_a_change_is_named() {
    let project = input(|value| value["projects"][1]["changes"] = json!(["/changes/gone"]));
    assert_eq!(code(&project).as_deref(), Some("keeper::map::unknown_path"));
    let closed = input(|value| value["projects"][1]["changes"] = json!(["/changes/old"]));
    assert_eq!(
        code(&closed).as_deref(),
        Some("keeper::map::unknown_path"),
        "not open"
    );
    let order =
        input(|value| value["projects"][0]["order"] = json!([["/changes/a", "/changes/x"]]));
    assert_eq!(code(&order).as_deref(), Some("keeper::map::unknown_path"));
    let link = input(|value| value["links"][0]["after"] = json!("/changes/x"));
    assert_eq!(code(&link).as_deref(), Some("keeper::map::unknown_path"));
    let finding = input(|value| value["findings"][0]["paths"] = json!(["/changes/x"]));
    assert_eq!(code(&finding).as_deref(), Some("keeper::map::unknown_path"));
}

#[test]
fn a_finding_of_a_closed_change_is_admitted_and_a_loose_link_may_name_any_record() {
    let loose = input(|value| {
        value["findings"] = json!([{"kind": "loose-link", "paths": ["/notes/x"],
            "evidence": "No change.", "fix": "none"}]);
    });
    assert_eq!(code(&loose), None);
}

#[test]
fn a_link_or_rule_that_names_an_unlisted_project_or_realm_is_refused() {
    let link = input(|value| value["links"][0]["project"] = json!("nowhere"));
    assert_eq!(code(&link).as_deref(), Some("keeper::map::unknown_project"));
    let names = input(|value| value["question_rules"]["names"]["guide"] = json!("nowhere"));
    assert_eq!(
        code(&names).as_deref(),
        Some("keeper::map::unknown_project")
    );
    let task = input(|value| value["question_rules"]["tasks"][0]["to"] = json!("nowhere"));
    assert_eq!(code(&task).as_deref(), Some("keeper::map::unknown_project"));
    let realm = input(|value| value["question_rules"]["localDefault"] = json!("nowhere"));
    assert_eq!(code(&realm).as_deref(), Some("keeper::map::unknown_realm"));
}

#[test]
fn a_finding_id_is_its_kind_and_the_sha256_of_its_paths_whatever_else_changes() {
    let paths = vec!["/changes/a".to_owned(), "/changes/b".to_owned()];
    let id = finding_id(Kind::StalePlan, &paths);
    let digest = Sha256::digest(b"/changes/a\n/changes/b");
    let expected = format!(
        "{:08x}",
        u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
    );
    assert_eq!(id, format!("stale-plan-{expected}"));
    assert_ne!(
        id,
        finding_id(Kind::StalePlan, &paths[..1]),
        "other paths, other id"
    );
    assert_eq!(finding_id(Kind::Stale, &paths).split_at(5).0, "stale");
    assert!(regex_like(&id), "{id} must satisfy the schema's id pattern");
}

/// `^[a-z]+(-[a-z]+)*-[0-9a-f]{8}$`, checked by hand.
fn regex_like(id: &str) -> bool {
    let (words, hex) = id.rsplit_once('-').unwrap();
    hex.len() == 8
        && hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && words
            .split('-')
            .all(|word| !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase()))
}

#[test]
fn a_commit_mounted_twice_is_deduped_to_its_shortest_path_and_one_mounted_once_is_not() {
    let mounts: Vec<(String, String)> = [
        ("/repos/notes", "c1"),
        ("/repos/team/repos/notes", "c1"),
        ("/repos/team", "c2"),
        ("/repos/other/repos/notes", "c1"),
    ]
    .into_iter()
    .map(|(path, commit)| (path.to_owned(), commit.to_owned()))
    .collect();
    let found = dedupe(&mounts);
    assert_eq!(
        found,
        BTreeMap::from([
            (
                "/repos/team/repos/notes".to_owned(),
                "/repos/notes".to_owned()
            ),
            (
                "/repos/other/repos/notes".to_owned(),
                "/repos/notes".to_owned()
            ),
        ])
    );
    assert!(dedupe(&mounts[2..3]).is_empty());
}
