//! Kept mount probes. A warm run opens no mounted repository whose stamp is
//! unchanged, opens one again after its stamp changes, and answers as a
//! cold run either way.

use std::path::Path;

use board_model::ObjectId;
use board_scenario::{Item, Scenario};

use super::decisions::query_as;
use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, board, task};

fn mount(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A library board, committed, whose config includes a file beside it.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let commit = library.commit(&[
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ("tasks/ok.json", &task(r#", "project": "/projects/p""#)),
    ]);
    library
        .write(".git/extra.config", Item::File("[user]\n\tname = one\n"))
        .write(
            ".git/config",
            Item::Appended("[include]\n\tpath = extra.config\n"),
        );
    (library, commit)
}

/// A change to make between two queries, and what it is.
type Change<'s> = (&'static str, Box<dyn Fn() + 's>);

/// How many mounted repositories a warm query of `scenario` through the
/// index in `cache` opened, after it answered as a cold one.
fn opened(scenario: &Scenario, cache: &Path) -> usize {
    query_as(scenario, cache, "test").repositories
}

#[test]
fn a_warm_query_of_an_unchanged_board_opens_no_mounted_repository_and_a_cold_one_opens_each() {
    let (inner, inner_commit) = library();
    let middle = Scenario::new();
    let middle_commit = middle.commit(&[
        (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ("inner.json", &mount(&inner.path(), inner_commit)),
    ]);
    let scenario = board();
    scenario.write(
        "middle.json",
        Item::File(&mount(&middle.path(), middle_commit)),
    );
    let cache = tempfile::tempdir().unwrap();
    assert_eq!(opened(&scenario, cache.path()), 2, "a cold index");
    assert_eq!(opened(&scenario, cache.path()), 0, "a warm index");
}

#[test]
fn a_build_that_names_no_build_opens_every_mounted_repository_each_time_and_its_named_twin_does_not()
 {
    for (build, warm) in [("", 1), ("test", 0)] {
        let (library, commit) = library();
        let scenario = board();
        scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
        let cache = tempfile::tempdir().unwrap();
        query_as(&scenario, cache.path(), build);
        let work = query_as(&scenario, cache.path(), build);
        assert_eq!(work.repositories, warm, "{build:?}");
    }
}

#[test]
fn after_each_change_to_a_mounted_repositorys_stamp_a_warm_query_opens_it_again_and_answers_as_a_cold_one()
 {
    let (library, commit) = library();
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
    let cache = tempfile::tempdir().unwrap();
    let changes: Vec<Change<'_>> = vec![
        (
            "its config rewritten",
            Box::new(|| {
                let origin = "[remote \"origin\"]\n\turl = https://example.com/lib.git\n";
                library.write(".git/config", Item::Appended(origin));
            }),
        ),
        (
            "a file its config includes rewritten",
            Box::new(|| {
                library.write(".git/extra.config", Item::File("[user]\n\tname = two\n"));
            }),
        ),
        (
            "a pack added",
            Box::new(|| {
                library.write(".git/objects/pack/pack-extra.keep", Item::File(""));
            }),
        ),
        (
            "an alternates file added",
            Box::new(|| {
                library.write(".git/objects/info/alternates", Item::File("/nowhere\n"));
            }),
        ),
    ];
    opened(&scenario, cache.path());
    assert_eq!(opened(&scenario, cache.path()), 0, "nothing changed");
    for (change, make) in changes {
        make();
        assert_eq!(opened(&scenario, cache.path()), 1, "{change}");
        assert_eq!(opened(&scenario, cache.path()), 0, "{change}, then nothing");
    }
}

#[test]
fn a_mount_whose_commit_is_lost_is_opened_every_run_and_answers_as_a_cold_one() {
    let (library, commit) = library();
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
    let cache = tempfile::tempdir().unwrap();
    opened(&scenario, cache.path());
    assert_eq!(opened(&scenario, cache.path()), 0);
    let hex = commit.to_string();
    library.write(
        &format!(".git/objects/{}/{}", &hex[..2], &hex[2..]),
        Item::Deleted,
    );
    for run in ["first", "second"] {
        assert_eq!(opened(&scenario, cache.path()), 1, "{run}");
    }
}

#[test]
fn a_mount_whose_repository_is_removed_answers_as_a_cold_one() {
    let (library, commit) = library();
    let scenario = board();
    scenario.write("lib.json", Item::File(&mount(&library.path(), commit)));
    let cache = tempfile::tempdir().unwrap();
    opened(&scenario, cache.path());
    assert_eq!(opened(&scenario, cache.path()), 0);
    library.write("", Item::Deleted);
    for run in ["first", "second"] {
        assert_eq!(opened(&scenario, cache.path()), 0, "{run}");
    }
}
