//! What `check` says of a symlink (docs/architecture/paths/symlinks.md).

use board_model::{Change, Code};
use board_scenario::Item;
use board_tree::Status;

use super::{board, check, mount_of, open};
use crate::check::{Checked, Detail};

#[test]
fn a_symlink_to_nothing_is_degraded_with_a_detail_and_hint_naming_its_text_and_its_twin_to_a_file_is_ok()
 {
    let scenario = board();
    scenario
        .write("notes/a.md", Item::File("A.\n"))
        .write("notes/l.md", Item::Symlink("a.md"))
        .write("notes/d.md", Item::Symlink("gone.md"));
    let report = check(&scenario);
    let entry = |path: &str| -> Checked {
        let found = report.entries.iter().find(|entry| entry.path == path);
        found.unwrap().clone()
    };
    let followed = entry("notes/l.md");
    assert_eq!((followed.status, followed.detail), (Status::Ok, None));
    let dangling = entry("notes/d.md");
    assert_eq!(dangling.status, Status::Degraded(Code::SymlinkDangling));
    let detail = Detail {
        pointer: None,
        reason: r#"is a symlink to "gone.md", which names nothing"#.to_owned(),
        failures: 1,
        at: None,
        mount: None,
        target: Some("gone.md".to_owned()),
    };
    assert_eq!(dangling.detail, Some(detail));
    assert_eq!(
        dangling.hint().unwrap(),
        "no file the board holds is at `gone.md`, which the symlink names (a file git ignores or one in `.git` is not held); create or track the file there, or change the symlink to name a file the board holds"
    );
}

#[test]
fn a_committed_symlink_stays_committed_when_its_file_changes_and_is_uncommitted_when_its_text_does()
{
    let scenario = board();
    let commit = scenario.commit_items(&[
        (b"notes/a.md", Item::File("A.\n")),
        (b"notes/b.md", Item::File("B.\n")),
        (b"notes/l.md", Item::Symlink("a.md")),
    ]);
    scenario
        .track(commit)
        .write("notes/a.md", Item::File("A, edited.\n"))
        .write("notes/b.md", Item::File("B.\n"))
        .write("notes/l.md", Item::Symlink("a.md"));
    let change = |scenario| {
        let report = check(scenario);
        let at = |path: &str| {
            let entry = report.entries.iter().find(|entry| entry.path == path);
            entry.unwrap().change
        };
        (at("notes/a.md"), at("notes/l.md"))
    };
    assert_eq!(change(&scenario), (Change::Uncommitted, Change::Committed));
    scenario
        .write("notes/l.md", Item::Deleted)
        .write("notes/l.md", Item::Symlink("b.md"));
    assert_eq!(
        change(&scenario),
        (Change::Uncommitted, Change::Uncommitted)
    );
}

#[test]
fn a_json_symlink_to_a_directory_is_read_as_its_text_and_its_twin_to_a_json_file_is_ok() {
    let scenario = board();
    scenario
        .write("notes/a.json", Item::File("{}"))
        .write("linked/dir.json", Item::Symlink("../notes"))
        .write("linked/file.json", Item::Symlink("../notes/a.json"));
    let report = check(&scenario);
    let entry = |path: &str| -> Checked {
        let found = report.entries.iter().find(|entry| entry.path == path);
        found.unwrap().clone()
    };
    assert_eq!(entry("linked/file.json").status, Status::Ok);
    let dir = entry("linked/dir.json");
    assert_eq!(dir.status, Status::Degraded(Code::JsonParse));
    assert_eq!(dir.detail.unwrap().target, None);
}

#[test]
fn a_relative_link_in_a_symlinked_note_resolves_from_the_symlinks_directory_and_in_its_target_from_the_targets()
 {
    let scenario = board();
    scenario
        .write("other/a.md", Item::File("[b](b.md)\n"))
        .write("notes/b.md", Item::File("B.\n"))
        .write("notes/l.md", Item::Symlink("../other/a.md"));
    let report = check(&scenario);
    let status = |path: &str| {
        let found = report.entries.iter().find(|entry| entry.path == path);
        found.unwrap().status
    };
    assert_eq!(status("notes/l.md"), Status::Ok);
    assert_eq!(status("other/a.md"), Status::Degraded(Code::RefDangling));
}

/// The directories of the board's own working tree that a cold `check`, a
/// warm one and `places` each list, with `links` symlinks made by `link`.
fn listed(links: usize, link: impl Fn(&board_scenario::Scenario, usize)) -> [usize; 3] {
    let scenario = board();
    for at in 0..links {
        link(&scenario, at);
    }
    let cache = tempfile::tempdir().unwrap();
    let board = open(&scenario);
    let repository = board.repository();
    let mut counts = [0; 3];
    for count in &mut counts[..2] {
        let before = repository.directories_listed();
        board.check(None, Some(cache.path())).unwrap();
        *count = repository.directories_listed() - before;
    }
    let before = repository.directories_listed();
    board
        .places(scenario.workdir(), Some(cache.path()))
        .unwrap();
    counts[2] = repository.directories_listed() - before;
    counts
}

#[test]
fn checking_symlinked_mount_pointers_lists_as_many_directories_however_many_there_are() {
    let mounted = board_scenario::Scenario::new();
    let commit = mounted.commit(&[("x.json", "{}")]);
    let pointer = mount_of(&mounted, commit);
    let link = |scenario: &board_scenario::Scenario, at: usize| {
        scenario
            .write(&format!("mounts/m{at}.json"), Item::File(&pointer))
            .write(
                &format!("links/l{at}.json"),
                Item::Symlink(&format!("../mounts/m{at}.json")),
            );
    };
    assert_eq!(listed(5, link), listed(25, link));
}

#[test]
fn indexing_symlinks_whose_role_differs_from_their_files_lists_as_many_directories_however_many_there_are()
 {
    // A `.json` symlink to a `.md` file is parsed as JSON, which the walk
    // that hashed the `.md` file did not do, so the refresh reads it.
    let link = |scenario: &board_scenario::Scenario, at: usize| {
        scenario
            .write(
                &format!("data/f{at}.md"),
                Item::File(&format!("{{\"n\": {at}}}")),
            )
            .write(
                &format!("links/l{at}.json"),
                Item::Symlink(&format!("../data/f{at}.md")),
            );
    };
    assert_eq!(listed(5, link), listed(25, link));
}
