//! Advancing a mount to the commit its `follow` ref names.

use board_model::{Code, Fields, LogicalPath, ObjectId, canonical_json, parse_json};
use board_scenario::{Item, Scenario};

use crate::mount::MountUpdateReport;
use crate::put::Expect;
use crate::tests::{board, open, task};

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
        .mount_update(&path(at), board_tree::LOCK_WAIT)
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
        canonical_json(&parse_json(mount(&library.path(), "main", new).as_bytes()).unwrap())
            .into_bytes();
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
                .mount_update(&path(refused), board_tree::LOCK_WAIT)
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
    let refused = open(&scenario)
        .mount_update(&path("/vendor/lib/inner"), board_tree::LOCK_WAIT)
        .unwrap_err();
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert_eq!(refused.refusal().fields["path"], "/vendor/lib");
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
    assert_eq!(
        head.message,
        "board mount update /vendor/lib\n\nBoard-Bulk: mount update /vendor/lib"
    );
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

#[test]
fn a_repo_record_moves_its_checkout_commit_to_its_follow_and_keeps_its_remote() {
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    let new = library.commit(&[("a.md", "two")]);
    let scenario = board();
    let record = format!(
        r#"{{"$type": "board/repo/v1", "remote": "example-org/lib", "checkout": {{"repo": "{}", "follow": "main", "commit": "{old}"}}}}"#,
        library.path()
    );
    scenario
        .write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        )
        .write("repos/lib.json", Item::File(&record));
    let report = update(&scenario, "/repos/lib").unwrap();
    assert_eq!((report.old, report.new, report.wrote), (old, new, true));
    let written = parse_json(&file(&scenario, "repos/lib.json")).unwrap();
    assert_eq!(written["checkout"]["commit"], new.to_string());
    assert_eq!(written["remote"], "example-org/lib");
    assert!(written.get("commit").is_none());
}

/// A library holding one commit, and a mount of it in each way a mount
/// cannot be read, by the code each is degraded with.
fn broken(library: &Scenario, pinned: ObjectId) -> [(String, Code); 2] {
    [
        (
            mount(&format!("{}/gone", library.path()), "main", pinned),
            Code::MountRepoMissing,
        ),
        (
            mount(&library.path(), "main", ObjectId::Sha1([9; 20])),
            Code::MountCommitMissing,
        ),
    ]
}

#[test]
fn get_of_a_mount_that_cannot_be_read_returns_its_record_and_get_beneath_it_refuses() {
    let library = Scenario::new();
    let pinned = library.commit(&[("a.md", "one")]);
    for (pointer, code) in broken(&library, pinned) {
        let scenario = board();
        scenario.write("vendor/lib.json", Item::File(&pointer));
        let board = open(&scenario);
        let report = board.get(&path("/vendor/lib")).unwrap();
        assert_eq!(report.bytes, pointer.as_bytes());
        assert_eq!(
            report.record.unwrap()["repo"],
            parse_json(pointer.as_bytes()).unwrap()["repo"]
        );
        let beneath = path("/vendor/lib/a.md");
        assert_eq!(board.get(&beneath).unwrap_err().code(), code);
        let paths = [path("/vendor/lib"), beneath];
        let each: Vec<_> = board.get_each(&paths).unwrap().collect();
        assert!(each[0].is_ok());
        assert_eq!(each[1].as_ref().unwrap_err().code(), code);
    }
}

#[test]
fn get_of_a_mount_that_can_be_read_is_not_a_record() {
    let (scenario, _library, ..) = moved();
    assert_eq!(
        open(&scenario)
            .get(&path("/vendor/lib"))
            .unwrap_err()
            .code(),
        Code::RecordNotARecord
    );
}

#[test]
fn a_put_of_a_mount_that_opens_replaces_one_that_cannot_be_read() {
    let library = Scenario::new();
    let pinned = library.commit(&[("a.md", "one")]);
    let good = mount(&library.path(), "main", pinned);
    for (pointer, _) in broken(&library, pinned) {
        let scenario = board();
        scenario
            .write(
                ".board/board.json",
                Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
            )
            .write("vendor/lib.json", Item::File(&pointer));
        let board = open(&scenario);
        let written = board
            .put(&path("/vendor/lib"), Expect::Any, good.as_bytes())
            .unwrap();
        assert_eq!(written.writes[0].degraded, None);
        assert_eq!(
            file(&scenario, "vendor/lib.json"),
            canonical_json(&parse_json(good.as_bytes()).unwrap()).into_bytes()
        );
        assert!(board.get(&path("/vendor/lib/a.md")).is_ok());
    }
}

#[test]
fn a_put_of_a_record_that_is_no_pointer_over_a_mount_that_cannot_be_read_is_refused() {
    let library = Scenario::new();
    let pinned = library.commit(&[("a.md", "one")]);
    for (pointer, code) in broken(&library, pinned) {
        let scenario = board();
        scenario.write("vendor/lib.json", Item::File(&pointer));
        let refused = open(&scenario)
            .put(&path("/vendor/lib"), Expect::Any, task("").as_bytes())
            .unwrap_err();
        assert_eq!(refused.code(), code);
    }
}

#[test]
fn a_put_of_a_mount_over_one_that_can_be_read_stays_refused() {
    let (scenario, library, old, _) = moved();
    let again = mount(&library.path(), "main", old);
    assert_eq!(
        open(&scenario)
            .put(&path("/vendor/lib"), Expect::Any, again.as_bytes())
            .unwrap_err()
            .code(),
        Code::PutThroughMount
    );
}

#[test]
fn a_mount_written_under_a_temporary_directory_says_so_and_one_elsewhere_does_not() {
    let library = Scenario::new();
    let pinned = library.commit(&[("a.md", "one")]);
    let scenario = board();
    scenario.write(
        ".board/board.json",
        Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
    );
    let board = open(&scenario);
    let written = |at: &str, repo: &str| {
        let pointer = mount(repo, "main", pinned);
        let report = board
            .put(&path(at), Expect::Any, pointer.as_bytes())
            .unwrap();
        report.writes[0].temporary.clone()
    };
    for repo in ["/tmp/a", "/private/tmp/b", "/var/folders/x/T/c"] {
        assert_eq!(written("/vendor/one", repo).as_deref(), Some(repo));
    }
    assert_eq!(written("/vendor/two", "/home/person/code/lib"), None);
    assert_eq!(written("/vendor/three", "/tmpfiles/lib"), None);
}
