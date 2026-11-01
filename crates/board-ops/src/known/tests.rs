//! Known boards: from a repository that a known board mounts, Board opens
//! that board, with the directory's logical path as the prefix, as git runs
//! from a subdirectory (docs/architecture.md, Boards and entries).

use std::path::{Path, PathBuf};

use board_model::Refusal;
use board_scenario::seed::mount_of;
use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use super::*;
use crate::tests::board;

/// A repository with a commit and a `docs` directory, to mount.
fn library() -> Scenario {
    let library = Scenario::new();
    library.commit(&[("x.json", "1")]);
    library.write("docs/a.md", Item::File("a"));
    library
}

/// A board that mounts `library` at `/repos/lib`.
fn mounting(library: &Scenario) -> Scenario {
    let scenario = board();
    let commit = library.commit(&[("x.json", "1")]);
    let mount = mount_of(&library.path(), commit);
    scenario.write("repos/lib.json", Item::File(&mount));
    scenario
}

/// An XDG config directory whose known boards file holds `text`, or no
/// file when `text` is `None`.
fn config(text: Option<&str>) -> Scenario {
    let xdg = Scenario::new();
    if let Some(text) = text {
        xdg.write("board/config.json", Item::File(text));
    }
    xdg
}

/// The known boards file listing `boards`.
fn listing(boards: &[&Scenario]) -> Scenario {
    let roots: Vec<String> = boards.iter().map(|board| board.path()).collect();
    config(Some(&json!({"boards": roots}).to_string()))
}

/// The known boards file naming `personal` as the personal board.
fn naming(personal: &Scenario) -> Scenario {
    config(Some(&json!({"personal": personal.path()}).to_string()))
}

/// The known boards file in `xdg`.
fn file(xdg: &Scenario) -> PathBuf {
    xdg.workdir().join("board/config.json")
}

/// The known boards `xdg`'s file names, read as an adapter reads it.
fn known(xdg: &Scenario) -> Result<KnownBoards, Box<Refusal>> {
    let file = file(xdg);
    KnownBoards::parse(&file, std::fs::read(&file))
}

/// Opens the board for `start` with `xdg`'s known boards and `cache`, and
/// gives the logical path it starts at, or the refusal.
fn opened(start: &Path, xdg: &Scenario, cache: Option<&Path>) -> Result<String, Box<Refusal>> {
    let (_, prefix) = open(start, None, cache, || known(xdg))?;
    Ok(match prefix {
        Some(Ok(path)) => path.to_string(),
        other => format!("{other:?}"),
    })
}

fn refused(start: &Path, xdg: &Scenario) -> Refusal {
    *opened(start, xdg, None).unwrap_err()
}

#[test]
fn a_mounted_repository_opens_its_board_and_an_unmounted_one_finds_no_board() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    assert_eq!(opened(library.workdir(), &xdg, None).unwrap(), "/repos/lib");
    let unmounted = Scenario::new();
    let lost = refused(unmounted.workdir(), &xdg);
    assert_eq!(lost.code.as_str(), "board::config::not_found");
    let empty = config(None);
    let lost = refused(unmounted.workdir(), &empty);
    assert_eq!(lost.code.as_str(), "board::config::not_found");
    let hint = format!(
        "; to run from a repository a board mounts, list that board in `{}` as \
         {{\"boards\": [\"<board directory>\"]}}",
        file(&empty).display()
    );
    let lost_hint = lost.hint.unwrap();
    assert!(lost_hint.ends_with(&hint), "{lost_hint}");
}

#[test]
fn a_linked_worktree_of_a_mounted_repository_opens_its_board() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let pinned = library.commit(&[("x.json", "1")]);
    // A linked worktree as `git worktree add --detach` lays it out.
    let linked = format!("{}/linked", library.path());
    let git_dir = format!("{}/.git/worktrees/linked", library.path());
    library
        .write(
            ".git/worktrees/linked/HEAD",
            Item::File(&format!("{pinned}\n")),
        )
        .write(".git/worktrees/linked/commondir", Item::File("../..\n"))
        .write(
            ".git/worktrees/linked/gitdir",
            Item::File(&format!("{linked}/.git\n")),
        )
        .write("linked/.git", Item::File(&format!("gitdir: {git_dir}\n")))
        .write("linked/docs/a.md", Item::File("a"));
    let docs = Path::new(&linked).join("docs");
    assert_eq!(opened(&docs, &xdg, None).unwrap(), "/repos/lib/docs");
}

#[test]
fn two_known_boards_mounting_the_repository_are_refused_and_one_is_admitted() {
    let library = library();
    let (one, two) = (mounting(&library), mounting(&library));
    let both = listing(&[&one, &two]);
    let refusal = refused(library.workdir(), &both);
    assert_eq!(refusal.code.as_str(), "board::config::ambiguous");
    for board in [&one, &two] {
        let named = format!("{} at /repos/lib", board.path());
        assert!(refusal.message.contains(&named), "{}", refusal.message);
    }
    let hint = refusal.hint.unwrap();
    let named = format!("`{}`", file(&both).display());
    assert!(hint.contains(&named), "{hint}");
    let alone = listing(&[&one]);
    assert_eq!(
        opened(library.workdir(), &alone, None).unwrap(),
        "/repos/lib"
    );
}

#[test]
fn a_missing_or_empty_known_boards_file_finds_no_board_and_one_listing_the_board_finds_it() {
    let library = library();
    let board = mounting(&library);
    for xdg in [config(None), config(Some("")), config(Some("{}"))] {
        let lost = refused(library.workdir(), &xdg);
        assert_eq!(lost.code.as_str(), "board::config::not_found");
    }
    let found = opened(library.workdir(), &listing(&[&board]), None);
    assert_eq!(found.unwrap(), "/repos/lib");
}

#[test]
fn a_known_boards_file_that_is_not_a_list_is_refused_and_a_list_is_admitted() {
    let library = library();
    let board = mounting(&library);
    let field = config(Some(&json!({"board": [board.path()]}).to_string()));
    let refusal = refused(library.workdir(), &field);
    assert_eq!(refusal.code.as_str(), "board::config::invalid");
    let prefix = format!("{} is not a list of known boards: ", file(&field).display());
    assert!(refusal.message.starts_with(&prefix), "{}", refusal.message);
    let hint = format!(
        "write `{}` as {{\"boards\": [\"<board directory>\"], \"personal\": \"<board directory>\"}}, either key optional, or remove it",
        file(&field).display()
    );
    assert_eq!(refusal.hint.unwrap(), hint);
    let list = config(Some(&json!({"boards": [board.path()]}).to_string()));
    assert_eq!(
        opened(library.workdir(), &list, None).unwrap(),
        "/repos/lib"
    );
}

#[test]
fn outside_a_repository_the_known_boards_are_not_read_and_inside_one_they_are() {
    let library = library();
    let outside = tempfile::tempdir().unwrap();
    let unread = open(outside.path(), None, None, || {
        panic!("read the known boards")
    });
    let lost = *unread.unwrap_err();
    assert_eq!(lost.code.as_str(), "board::config::not_found");
    assert!(lost.hint.unwrap().starts_with("run `git init` first"));
    let xdg = config(Some("not json"));
    let refusal = refused(library.workdir(), &xdg);
    assert_eq!(refusal.code.as_str(), "board::config::invalid");
    let board = mounting(&library);
    let found = opened(library.workdir(), &listing(&[&board]), None);
    assert_eq!(found.unwrap(), "/repos/lib");
}

#[test]
fn a_known_boards_file_that_cannot_be_read_is_refused_and_a_missing_one_lists_none() {
    let library = library();
    let unreadable = Scenario::new();
    unreadable.write("board/config.json/inner", Item::File("a directory"));
    let refusal = refused(library.workdir(), &unreadable);
    assert_eq!(refusal.code.as_str(), "board::config::invalid");
    let missing = refused(library.workdir(), &config(None));
    assert_eq!(missing.code.as_str(), "board::config::not_found");
    let board = mounting(&library);
    let found = opened(library.workdir(), &listing(&[&board]), None);
    assert_eq!(found.unwrap(), "/repos/lib");
}

#[test]
fn one_board_listed_under_several_spellings_is_admitted_and_two_boards_are_ambiguous() {
    let library = library();
    let (one, two) = (mounting(&library), mounting(&library));
    let canonical = one.workdir().canonicalize().unwrap();
    let spellings = json!({"boards": [
        one.path(),
        canonical,
        format!("{}/", one.path()),
        one.workdir().join(".board"),
    ]});
    let xdg = config(Some(&spellings.to_string()));
    assert_eq!(opened(library.workdir(), &xdg, None).unwrap(), "/repos/lib");
    let refusal = refused(library.workdir(), &listing(&[&one, &two]));
    assert_eq!(refusal.code.as_str(), "board::config::ambiguous");
}

#[test]
fn known_boards_that_do_not_mount_the_repository_are_named_as_read_and_one_that_does_is_admitted() {
    let library = library();
    let board = mounting(&library);
    let nowhere = library.workdir().join("nowhere");
    let xdg = config(Some(
        &json!({"boards": [nowhere, board.path()]}).to_string(),
    ));
    let unmounted = Scenario::new();
    let lost = refused(unmounted.workdir(), &xdg);
    assert_eq!(lost.code.as_str(), "board::config::not_found");
    let read = format!(
        "; the known boards in `{}` were read and none has a mount or repo record for this \
         repository, and these could not be opened or refreshed: {}",
        file(&xdg).display(),
        nowhere.display()
    );
    assert!(lost.message.ends_with(&read), "{}", lost.message);
    let hint = lost.hint.unwrap();
    let put = "put a `board/repo/v1` record whose `remote` is this repository's origin";
    assert!(hint.starts_with(put), "{hint}");
    let all_opened = refused(unmounted.workdir(), &listing(&[&board]));
    let none = "none has a mount or repo record for this repository";
    assert!(all_opened.message.ends_with(none), "{}", all_opened.message);
    assert_eq!(opened(library.workdir(), &xdg, None).unwrap(), "/repos/lib");
}

#[test]
fn a_board_that_cannot_be_opened_is_passed_over() {
    let library = library();
    let board = mounting(&library);
    let nowhere = library.workdir().join("nowhere");
    let xdg = config(Some(
        &json!({"boards": [nowhere, board.path()]}).to_string(),
    ));
    assert_eq!(opened(library.workdir(), &xdg, None).unwrap(), "/repos/lib");
}

#[test]
fn discovery_answers_from_a_warm_index_as_from_a_cold_one_after_the_mount_moves() {
    let library = library();
    let board = mounting(&library);
    let xdg = listing(&[&board]);
    let cache = tempfile::tempdir().unwrap();
    let warm = || opened(library.workdir(), &xdg, Some(cache.path())).unwrap();
    let cold = || opened(library.workdir(), &xdg, None).unwrap();
    assert_eq!(warm(), "/repos/lib");
    assert_eq!(warm(), cold());
    let commit = library.commit(&[("x.json", "1")]);
    board.write("repos/lib.json", Item::Deleted).write(
        "vendor/lib.json",
        Item::Fresh(&mount_of(&library.path(), commit)),
    );
    assert_eq!(warm(), "/vendor/lib");
    assert_eq!(cold(), "/vendor/lib");
}

/// The refusal of `--personal` with `xdg`'s known boards.
fn no_personal(xdg: &Scenario) -> Refusal {
    *known(xdg).unwrap().personal(None).unwrap_err()
}

#[test]
fn personal_is_refused_when_the_known_boards_file_names_no_personal_board_and_admitted_when_it_does()
 {
    let personal = board();
    for xdg in [config(None), listing(&[&personal])] {
        let refusal = no_personal(&xdg);
        assert_eq!(refusal.code.as_str(), "board::config::not_found");
        let shown = file(&xdg).display().to_string();
        assert_eq!(
            refusal.message,
            format!("`{shown}` names no personal board")
        );
        let hint = format!(
            "name the personal board in `{shown}` as {{\"personal\": \"<board directory>\"}}, the directory that holds .board/board.json"
        );
        assert_eq!(refusal.hint.unwrap(), hint);
    }
    let root = known(&naming(&personal)).unwrap().personal(None).unwrap();
    assert_eq!(root, personal.workdir());
}

#[test]
fn personal_naming_anything_but_a_boards_root_is_refused_and_the_root_is_admitted() {
    let library = library();
    let work = mounting(&library);
    let missing = work.workdir().join("missing");
    let below = work.workdir().join("projects");
    for named in [missing.as_path(), library.workdir(), below.as_path()] {
        let known_boards = json!({"boards": [work.path()], "personal": named});
        let xdg = config(Some(&known_boards.to_string()));
        let refusal = no_personal(&xdg);
        assert_eq!(refusal.code.as_str(), "board::config::not_found");
        let message = format!(
            "`personal` in `{}` names {}, which is not the root of a board",
            file(&xdg).display(),
            named.display()
        );
        assert_eq!(refusal.message, message);
        let hint = format!("name the personal board in `{}` as", file(&xdg).display());
        assert!(refusal.hint.unwrap().starts_with(&hint));
    }
    let known_boards = json!({"boards": [work.path()], "personal": work.path()});
    let xdg = config(Some(&known_boards.to_string()));
    let root = known(&xdg).unwrap().personal(None).unwrap();
    assert_eq!(root, work.workdir());
}

#[test]
fn a_personal_board_whose_board_json_is_invalid_is_refused_as_invalid_and_a_valid_one_is_admitted()
{
    let personal = board();
    let xdg = naming(&personal);
    personal.write(".board/board.json", Item::File(r#"{"x": 1}"#));
    assert_eq!(no_personal(&xdg).code.as_str(), "board::config::invalid");
    let valid = r#"{"$type": "board/config/v1"}"#;
    personal.write(".board/board.json", Item::File(valid));
    let root = known(&xdg).unwrap().personal(None).unwrap();
    assert_eq!(root, personal.workdir());
}

#[test]
fn a_relative_personal_path_is_read_against_the_known_boards_files_directory() {
    let personal = board();
    let name = personal.workdir().file_name().unwrap();
    let relative = Path::new("../..").join(name);
    let xdg = config(Some(&json!({"personal": relative}).to_string()));
    assert_eq!(xdg.workdir().parent(), personal.workdir().parent());
    let root = known(&xdg).unwrap().personal(None).unwrap();
    assert_eq!(
        root.canonicalize().unwrap(),
        personal.workdir().canonicalize().unwrap()
    );
}

#[test]
fn a_day_start_that_is_not_hh_mm_is_refused_and_one_that_is_or_is_null_is_admitted() {
    for start in ["7am", "24:00", "07:60", "7:00"] {
        let xdg = config(Some(&json!({"day_start": start}).to_string()));
        let refusal = *known(&xdg).unwrap_err();
        assert_eq!(refusal.code.as_str(), "board::config::invalid", "{start}");
        let reason = format!("day_start {start:?} is not HH:MM, such as \"07:00\"");
        assert!(refusal.message.contains(&reason), "{}", refusal.message);
    }
    for (start, wanted) in [(json!("07:00"), Some("07:00")), (Value::Null, None)] {
        let xdg = config(Some(&json!({"day_start": start}).to_string()));
        assert_eq!(known(&xdg).unwrap().day_start(), wanted);
    }
}

/// A clone with `url` as its `origin` and a `docs` directory.
fn clone_of(url: &str) -> Scenario {
    let clone = library();
    let remote = format!("[remote \"origin\"]\n\turl = {url}\n");
    clone.write(".git/config", Item::Appended(&remote));
    clone
}

/// A board holding a repo record with no checkout for `remote` at `at`.
fn recording(remote: &str, at: &str) -> Scenario {
    let scenario = board();
    let record = json!({"$type": "board/repo/v1", "remote": remote}).to_string();
    scenario.write(at, Item::File(&record));
    scenario
}

#[test]
fn an_unmounted_clone_runs_with_its_repo_records_path_and_one_with_no_record_finds_no_board() {
    let clone = clone_of("git@github.com:example-org/lib.git");
    let stranger = clone_of("git@github.com:example-org/other.git");
    let board = recording("example-org/lib", "repos/lib.json");
    let xdg = listing(&[&board]);
    assert_eq!(opened(clone.workdir(), &xdg, None).unwrap(), "/repos/lib");
    let docs = clone.workdir().join("docs");
    assert_eq!(opened(&docs, &xdg, None).unwrap(), "/repos/lib");
    let lost = refused(stranger.workdir(), &xdg);
    assert_eq!(lost.code.as_str(), "board::config::not_found");
    assert!(lost.hint.unwrap().contains("board/repo/v1"));
}

#[test]
fn two_known_boards_with_a_repo_record_for_one_remote_are_ambiguous_and_one_is_admitted() {
    let clone = clone_of("https://github.com/example-org/lib.git");
    let one = recording("example-org/lib", "repos/lib.json");
    let two = recording("example-org/lib", "work/lib.json");
    let refusal = refused(clone.workdir(), &listing(&[&one, &two]));
    assert_eq!(refusal.code.as_str(), "board::config::ambiguous");
    assert!(
        refusal.message.contains("mount or repo record"),
        "{}",
        refusal.message
    );
    let admitted = opened(clone.workdir(), &listing(&[&one]), None).unwrap();
    assert_eq!(admitted, "/repos/lib");
}

#[test]
fn a_board_that_mounts_the_clone_wins_over_its_own_repo_record_by_remote() {
    let library = clone_of("git@github.com:example-org/lib.git");
    let commit = library.commit(&[("x.json", "1")]);
    let scenario = board();
    let record = json!({"$type": "board/repo/v1", "remote": "example-org/lib",
        "checkout": {"repo": library.path(), "follow": "main", "commit": commit.to_string()}});
    scenario.write("repos/lib.json", Item::File(&record.to_string()));
    let xdg = listing(&[&scenario]);
    let docs = library.workdir().join("docs");
    assert_eq!(opened(&docs, &xdg, None).unwrap(), "/repos/lib/docs");
}

#[test]
fn a_clone_one_board_mounts_runs_there_though_another_has_its_repo_record() {
    let library = clone_of("git@github.com:example-org/lib.git");
    let mounted = mounting(&library);
    let recorded = recording("example-org/lib", "repos/lib.json");
    let both = listing(&[&recorded, &mounted]);
    let docs = library.workdir().join("docs");
    let (_, prefix) = open(&docs, None, None, || known(&both)).unwrap();
    assert_eq!(prefix.unwrap().unwrap().as_str(), "/repos/lib/docs");
}
