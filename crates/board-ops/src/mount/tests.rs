//! Advancing a mount to the commit its `follow` ref names.

use board_model::{Code, Fields, LogicalPath, ObjectId, canonical_json, parse_json};
use board_scenario::{Item, Scenario};

use crate::mount::MountUpdateReport;
use crate::tests::{board, open};

fn mount(repo: &str, follow: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "{follow}", "commit": "{commit}"}}"#
    )
}

fn path(text: &str) -> LogicalPath {
    text.parse().unwrap()
}

/// The test board, set not to commit, with `vendor/lib` mounting a library
/// whose `main` has moved on from the mounted commit. Returns the board,
/// the library, and the old and new commits.
fn moved() -> (Scenario, Scenario, ObjectId, ObjectId) {
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    let new = library.commit(&[("a.md", "two")]);
    let scenario = board();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write(
            "vendor/lib.json",
            Item::File(&mount(&library.path(), "main", old)),
        );
    (scenario, library, old, new)
}

fn update(scenario: &Scenario, at: &str) -> Result<MountUpdateReport, Code> {
    open(scenario)
        .mount_update(&path(at))
        .map_err(|error| error.code())
}

fn file(scenario: &Scenario, physical: &str) -> Vec<u8> {
    std::fs::read(scenario.workdir().join(physical)).unwrap()
}

#[test]
fn a_mount_moves_to_its_follow_and_is_written_in_canonical_form() {
    let (scenario, library, old, new) = moved();
    let report = update(&scenario, "/vendor/lib").unwrap();
    assert_eq!(
        report,
        MountUpdateReport {
            path: path("/vendor/lib"),
            old,
            new,
            wrote: true,
        }
    );
    let expected =
        canonical_json(&parse_json(mount(&library.path(), "main", new).as_bytes()).unwrap());
    assert_eq!(file(&scenario, "vendor/lib.json"), expected);
    let checked = open(&scenario).check(None, None).unwrap();
    let degraded: Vec<_> = checked
        .entries
        .iter()
        .filter(|entry| entry.status != board_tree::Status::Ok)
        .map(|entry| (&entry.path, entry.status))
        .collect();
    assert_eq!(degraded, []);
}

#[test]
fn a_mount_already_at_its_follow_is_left_as_it_is() {
    let (scenario, library, _, new) = moved();
    let current = mount(&library.path(), "main", new);
    scenario.write("vendor/lib.json", Item::File(&current));
    let report = update(&scenario, "/vendor/lib").unwrap();
    assert_eq!((report.old, report.new, report.wrote), (new, new, false));
    assert_eq!(
        file(&scenario, "vendor/lib.json"),
        current.as_bytes(),
        "not even rewritten in canonical form"
    );
}

#[test]
fn a_missing_follow_ref_is_refused_and_an_existing_one_is_admitted() {
    let (scenario, library, old, _) = moved();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), "gone", old)),
    );
    assert_eq!(
        update(&scenario, "/vendor/lib").unwrap_err(),
        Code::MountFollowMissing
    );
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), "refs/heads/main", old)),
    );
    assert!(update(&scenario, "/vendor/lib").unwrap().wrote);
}

#[test]
fn a_missing_repository_is_refused() {
    let (scenario, _library, old, _) = moved();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount("/nowhere/at/all", "main", old)),
    );
    assert_eq!(
        update(&scenario, "/vendor/lib").unwrap_err(),
        Code::MountRepoMissing
    );
}

#[test]
fn a_mount_whose_commit_is_gone_still_moves_to_its_follow() {
    let (scenario, library, _, new) = moved();
    let gone = ObjectId::from_hex(&"1".repeat(40)).unwrap();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), "main", gone)),
    );
    let report = update(&scenario, "/vendor/lib").unwrap();
    assert_eq!((report.old, report.new, report.wrote), (gone, new, true));
}

#[test]
fn a_path_that_is_not_a_mount_is_refused_and_a_mount_is_admitted() {
    let (scenario, _library, _, _) = moved();
    scenario.write(
        "vendor/alias.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/vendor/lib"}"#),
    );
    // A link to the mount is not followed, so it is not a mount.
    for refused in ["/vendor/alias", "/projects/p"] {
        assert_eq!(
            update(&scenario, refused).unwrap_err(),
            Code::MountInvalid,
            "{refused}"
        );
        assert_eq!(
            open(&scenario)
                .mount_update(&path(refused))
                .unwrap_err()
                .refusal()
                .fields,
            Fields::at(&path(refused)).json()
        );
    }
    assert_eq!(
        update(&scenario, "/vendor").unwrap_err(),
        Code::RecordNotARecord
    );
    assert_eq!(
        update(&scenario, "/vendor/none").unwrap_err(),
        Code::PathNotFound
    );
    // A link before the last segment is followed.
    scenario.write(
        "v.json",
        Item::File(r#"{"$type": "board/mount/v1", "path": "/vendor"}"#),
    );
    let report = update(&scenario, "/v/lib").unwrap();
    assert_eq!((report.path, report.wrote), (path("/v/lib"), true));
}

#[test]
fn a_mount_under_a_name_git_keeps_is_refused_and_its_twin_is_admitted() {
    let (scenario, library, old, _) = moved();
    let pinned = mount(&library.path(), "main", old);
    scenario
        .write("GIT~1/lib.json", Item::File(&pinned))
        .write("GIT~2/lib.json", Item::File(&pinned));
    assert_eq!(
        update(&scenario, "/GIT~1/lib").unwrap_err(),
        Code::PathMalformed
    );
    assert_eq!(file(&scenario, "GIT~1/lib.json"), pinned.as_bytes());
    assert!(update(&scenario, "/GIT~2/lib").unwrap().wrote);
}

#[test]
fn a_mount_inside_a_mounted_board_is_refused() {
    let inner = Scenario::new();
    let inner_commit = inner.commit(&[("x.json", r#"{"$type": "lr/project/v1"}"#)]);
    let library = Scenario::new();
    let commit = library.commit(&[("inner.json", &mount(&inner.path(), "main", inner_commit))]);
    let scenario = board();
    scenario.write(
        "vendor/lib.json",
        Item::File(&mount(&library.path(), "main", commit)),
    );
    assert_eq!(
        update(&scenario, "/vendor/lib/inner").unwrap_err(),
        Code::PutThroughMount
    );
    assert!(!update(&scenario, "/vendor/lib").unwrap().wrote);
}

#[test]
fn a_committing_update_commits_the_pointer() {
    let (scenario, _library, _, new) = moved();
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1"}"#),
        )
        .write(
            ".git/config",
            Item::Appended("[user]\n\tname = Board Test\n\temail = board@example.com\n"),
        );
    assert!(update(&scenario, "/vendor/lib").unwrap().wrote);
    let head = scenario.head().unwrap();
    assert_eq!(head.message, "board mount update /vendor/lib");
    assert_eq!(head.files.keys().collect::<Vec<_>>(), ["vendor/lib.json"]);
    assert!(
        String::from_utf8(head.files["vendor/lib.json"].clone())
            .unwrap()
            .contains(&new.to_string())
    );
}

#[test]
fn a_mount_update_refused_as_stale_names_what_it_found_and_one_refused_otherwise_does_not() {
    let blob = ObjectId::Sha1([1; 20]);
    let stale = crate::MountError::Put(crate::PutError::Stale {
        path: path("/vendor/lib"),
        found: Some(blob),
    })
    .refusal();
    assert_eq!(stale.code, board_model::RefusalCode::Board(Code::PutStale));
    assert_eq!(
        serde_json::Value::Object(stale.fields),
        serde_json::json!({"path": "/vendor/lib", "blob": blob.to_string(), "found": blob.to_string()})
    );
    let other = crate::MountError::Put(crate::PutError::TooManyLines).refusal();
    assert_eq!(
        other.code,
        board_model::RefusalCode::Board(Code::PutInvalid)
    );
    assert!(other.fields.is_empty());
}
