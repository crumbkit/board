//! How a call finds notes repos and routes a note to one. These were the
//! routing tests of `test_repos.py`; its tests of the write loop against a
//! refusing repo are the notes crate's tests against a real mount.

use super::*;

const EXAMPLE_ORG: &str = "https://github.com/example-org/home.git";
const BOARD_REMOTE: &str = "git@github.com:crumbkit/board.git";

/// Whether `path` is an inbox note named for a time, such as
/// `inbox/2026-10-25-093000.md`.
fn inboxed(path: &str) -> bool {
    let Some(stem) = path
        .strip_prefix("inbox/")
        .and_then(|rest| rest.strip_suffix(".md"))
    else {
        return false;
    };
    stem.len() == 17
        && (stem.bytes().enumerate()).all(|(at, byte)| {
            [4, 7, 10].contains(&at) == (byte == b'-') && (byte == b'-' || byte.is_ascii_digit())
        })
}

#[test]
fn a_mount_without_daily_is_not_a_notes_repo_and_one_with_it_is() {
    let notes = Notes::new();
    let plain = notes.another("plain");
    git(plain.workdir(), &["rm", "-rq", "daily"]);
    git(plain.workdir(), &["commit", "-qm", "no daily"]);
    notes.mount("plain", &plain);
    let refused = notes.refused("daily", &["--repo", "/repos/plain"]);
    assert_eq!(refused["code"], "notes::repo::not_found");
    assert_eq!(refused["fields"]["repos"], json!(["/repos/notes-personal"]));
    notes
        .call("daily", &["--repo", "/repos/notes-personal"])
        .unwrap();
    let only = notes.call("daily", &[]).unwrap();
    assert_eq!(
        only["repo"], "/repos/notes-personal",
        "the only notes repo needs no --repo"
    );
}

#[test]
fn a_board_without_a_notes_repo_is_refused_and_one_with_its_own_daily_is_its_notes_repo() {
    let notes = Notes::new();
    let board = notes.board.workdir();
    git(board, &["rm", "-q", "repos/notes-personal.json"]);
    git(board, &["commit", "-qm", "unmount"]);
    let refused = notes.refused("add", &["--text", "Lost."]);
    assert_eq!(refused["code"], "notes::repo::not_found");
    assert_eq!(refused["fields"]["repos"], json!([]));
    layout(&notes.board);
    commit(board);
    let output = notes.call("add", &["--text", "Agents only."]).unwrap();
    let head = git(board, &["rev-parse", "HEAD"]);
    assert_eq!(
        (&output["repo"], &output["commit"], &output["unpushed"]),
        (&json!("/"), &json!(head), &json!("notes::repo::not_synced"))
    );
    let path = output["path"].as_str().unwrap();
    let changed = git(board, &["show", "--name-only", "--format=", &head]);
    assert_eq!(changed, path.trim_start_matches('/'));
    let read = notes.call("daily", &["--repo", "/"]).unwrap();
    assert_eq!(
        (&read["path"], &read["commit"], &read["unsynced"]),
        (
            &output["path"],
            &Value::Null,
            &json!("notes::repo::not_synced")
        )
    );
    assert!(read["text"].as_str().unwrap().ends_with("Agents only.\n"));
}

#[test]
fn a_note_goes_to_the_one_repo_covering_the_remote_and_otherwise_to_the_personal_inbox() {
    let notes = Notes::new();
    let work = notes.another("notes-work");
    notes.cover(&work, "notes-work", &["Example-Org/*"], false);
    notes.cover(&notes.clone, "notes-personal", &["crumbkit/notes"], true);
    let routed = notes
        .call("add", &["--text", "Deploy.", "--remote", EXAMPLE_ORG])
        .unwrap();
    assert_eq!(
        (&routed["repo"], &routed["candidates"]),
        (&json!("/repos/notes-work"), &Value::Null)
    );
    assert!(
        routed["path"]
            .as_str()
            .unwrap()
            .starts_with("/repos/notes-work/daily/")
    );
    let flags = [
        "--text",
        "Rename.",
        "--remote",
        BOARD_REMOTE,
        "--topic",
        "Board: names",
    ];
    let inbox = notes.call("add", &flags).unwrap();
    assert_eq!(
        (&inbox["repo"], &inbox["candidates"]),
        (&json!("/repos/notes-personal"), &json!([]))
    );
    let path = Notes::note(&inbox);
    assert!(inboxed(&path), "{path}");
    let frontmatter =
        "---\nremote: \"crumbkit/board\"\ncandidates: []\ntopic: \"Board: names\"\n---\n\n";
    let text = notes.remote_text(&path);
    let rest = text.strip_prefix(frontmatter).unwrap();
    assert!(section(&format!("{rest}\n"), "Rename.\n"), "{text}");
}

#[test]
fn a_note_two_repos_cover_goes_to_the_inbox_naming_both_and_one_repo_covering_it_takes_it() {
    let notes = Notes::new();
    let work = notes.another("notes-work");
    notes.cover(&work, "notes-work", &["example-org/*"], false);
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let output = notes
        .call("add", &["--text", "Mine.", "--remote", "example-org/home"])
        .unwrap();
    assert_eq!(output["repo"], "/repos/notes-work");
    let home = notes.another("notes-home");
    notes.cover(&home, "notes-home", &["example-org/home"], false);
    let output = notes
        .call("add", &["--text", "Whose?", "--remote", "example-org/home"])
        .unwrap();
    let both = json!(["/repos/notes-home", "/repos/notes-work"]);
    assert_eq!(
        (&output["repo"], &output["candidates"]),
        (&json!("/repos/notes-personal"), &both)
    );
    let text = notes.read(&Notes::note(&output));
    assert!(
        text.contains("candidates: [\"/repos/notes-home\", \"/repos/notes-work\"]"),
        "{text}"
    );
}

#[test]
fn without_a_personal_repo_an_uncovered_note_is_refused_and_with_one_it_goes_to_the_inbox() {
    let notes = Notes::new();
    let _work = notes.another("notes-work");
    let head = notes.head();
    let refused = notes.refused("add", &["--text", "Where?"]);
    assert_eq!(refused["code"], "notes::repo::no_personal");
    assert_eq!(refused["fields"]["personal"], json!([]));
    assert_eq!(notes.head(), head, "nothing was written");
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let output = notes.call("add", &["--text", "Where?"]).unwrap();
    let path = Notes::note(&output);
    assert!(path.starts_with("inbox/"), "{path}");
    assert!(
        !notes.read(&path).contains("remote:"),
        "no remote was given"
    );
}

#[test]
fn daily_reads_the_note_of_the_repo_covering_the_remote_and_refuses_one_no_repo_covers() {
    let notes = Notes::new();
    let work = notes.another("notes-work");
    notes.cover(&work, "notes-work", &["example-org/*"], false);
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let covered = notes.call("daily", &["--remote", EXAMPLE_ORG]).unwrap();
    assert_eq!(covered["repo"], "/repos/notes-work");
    let refused = notes.refused("daily", &["--remote", BOARD_REMOTE]);
    assert_eq!(
        refused["code"], "notes::repo::ambiguous",
        "the personal note is never read by default"
    );
    assert_eq!(refused["fields"]["candidates"], json!([]));
    let named = notes.call(
        "daily",
        &["--remote", BOARD_REMOTE, "--repo", "/repos/notes-personal"],
    );
    assert_eq!(named.unwrap()["repo"], "/repos/notes-personal");
}

#[test]
fn daily_without_a_remote_reads_the_personal_note_and_one_with_an_uncovered_remote_is_refused() {
    let notes = Notes::new();
    let work = notes.another("notes-work");
    notes.cover(&work, "notes-work", &["example-org/*"], false);
    notes.cover(&notes.clone, "notes-personal", &[], true);
    assert_eq!(
        notes.call("daily", &[]).unwrap()["repo"],
        "/repos/notes-personal"
    );
    let checkout = notes.checkout(BOARD_REMOTE);
    let refused = notes
        .call_in("daily", &[], UTC, Some(&checkout))
        .unwrap_err();
    assert_eq!(refused["code"], "notes::repo::ambiguous");
}

#[test]
fn the_only_repo_covering_something_refuses_a_call_without_a_remote_and_takes_one_it_covers() {
    let notes = Notes::new();
    notes.cover(&notes.clone, "notes-personal", &["example-org/*"], false);
    let refused = notes.refused("daily", &[]);
    assert_eq!(
        refused["code"], "notes::repo::ambiguous",
        "no remote may be a missing origin"
    );
    let refused = notes.refused("add", &["--text", "Where?"]);
    assert_eq!(refused["code"], "notes::repo::no_personal");
    let daily = notes.call("daily", &["--remote", EXAMPLE_ORG]).unwrap();
    assert_eq!(daily["repo"], "/repos/notes-personal");
}

#[test]
fn the_only_repo_without_a_record_serves_a_call_without_a_remote_and_refuses_one_with_a_remote() {
    let notes = Notes::new();
    assert_eq!(
        notes.call("daily", &[]).unwrap()["repo"],
        "/repos/notes-personal"
    );
    let refused = notes.refused("daily", &["--remote", EXAMPLE_ORG]);
    assert_eq!(
        refused["code"], "notes::repo::ambiguous",
        "a repo without a record covers nothing"
    );
    let refused = notes.refused("add", &["--text", "Where?", "--remote", EXAMPLE_ORG]);
    assert_eq!(refused["code"], "notes::repo::no_personal");
}

#[test]
fn a_remotes_credential_is_never_stored_or_shown_and_its_owner_and_name_are() {
    let notes = Notes::new();
    let tokened = "https://x-access-token:SECRET@github.com/crumbkit/board.git";
    let _work = notes.another("notes-work");
    let refused = notes.refused("add", &["--text", "Where?", "--remote", tokened]);
    assert_eq!(refused["code"], "notes::repo::no_personal");
    assert!(!refused.to_string().contains("SECRET"), "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("crumbkit/board")
    );
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let output = notes
        .call("add", &["--text", "Where?", "--remote", tokened])
        .unwrap();
    let text = notes.read(&Notes::note(&output));
    assert_eq!(
        (
            text.contains("SECRET"),
            text.contains("remote: \"crumbkit/board\"")
        ),
        (false, true)
    );
}

#[test]
fn a_note_is_routed_by_the_origin_of_the_directory_board_runs_in_and_remote_overrides_it() {
    let notes = Notes::new();
    let work = notes.another("notes-work");
    notes.cover(&work, "notes-work", &["example-org/*"], false);
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let home = notes.checkout(EXAMPLE_ORG);
    let routed = notes
        .call_in("add", &["--text", "From home."], UTC, Some(&home))
        .unwrap();
    assert_eq!(
        (&routed["repo"], &routed["candidates"]),
        (&json!("/repos/notes-work"), &Value::Null)
    );
    let flags = ["--text", "Elsewhere.", "--remote", BOARD_REMOTE];
    let overridden = notes.call_in("add", &flags, UTC, Some(&home)).unwrap();
    assert_eq!(
        (&overridden["repo"], &overridden["candidates"]),
        (&json!("/repos/notes-personal"), &json!([]))
    );
    assert!(
        notes
            .read(&Notes::note(&overridden))
            .contains("remote: \"crumbkit/board\"")
    );
}

#[test]
fn the_only_notes_repo_takes_a_note_it_covers_and_refuses_one_its_record_excludes() {
    let notes = Notes::new();
    notes.cover(&notes.clone, "notes-personal", &["example-org/*"], false);
    let head = notes.head();
    let elsewhere = notes.checkout(BOARD_REMOTE);
    let refused = notes.call_in("add", &["--text", "Not for work."], UTC, Some(&elsewhere));
    assert_eq!(refused.unwrap_err()["code"], "notes::repo::no_personal");
    assert_eq!(notes.head(), head, "nothing was written");
    let work = notes.checkout(EXAMPLE_ORG);
    let output = notes
        .call_in("add", &["--text", "For work."], UTC, Some(&work))
        .unwrap();
    assert_eq!(
        (&output["repo"], &output["candidates"]),
        (&json!("/repos/notes-personal"), &Value::Null)
    );
}

#[test]
fn a_record_without_its_schema_is_refused_by_name_and_one_with_it_is_admitted() {
    let notes = Notes::new();
    let _work = notes.another("notes-work");
    let record = r#"{"$type": "notes/repo/v1", "covers": [], "personal": true}"#;
    notes.clone.write("notes-repo.json", Item::File(record));
    commit(notes.clone.workdir());
    let pinned = notes.run(&["mount", "update", "/repos/notes-personal"], UTC, None);
    assert_eq!(pinned.code, 0, "{}", text(&pinned.stderr));
    let refused = notes.refused("add", &["--text", "Where?"]);
    assert_eq!(refused["code"], "notes::repo::invalid_record");
    assert_eq!(
        refused["fields"]["path"],
        "/repos/notes-personal/notes-repo"
    );
    assert_eq!(refused["fields"]["code"], "board::record::unknown_type");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("plugin enable notes")
    );
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let output = notes.call("add", &["--text", "Where?"]).unwrap();
    assert_eq!(output["repo"], "/repos/notes-personal");
}

#[test]
fn an_inbox_note_is_written_whole_when_the_clone_cannot_sync_and_a_daily_note_is_parked() {
    let notes = Notes::new();
    let _work = notes.another("notes-work");
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let path = "daily/2020-01-01.md";
    notes.clone.write(path, Item::File("Old.\n"));
    commit(notes.clone.workdir());
    git(notes.clone.workdir(), &["push", "-q"]);
    notes.phone_appends(path, "From the phone.\n");
    notes
        .clone
        .write(path, Item::File("Old.\nTyping in Reflect"));
    let flags = ["--text", "Whose?", "--remote", BOARD_REMOTE, "--topic", "T"];
    let inbox = notes.call("add", &flags).unwrap();
    assert_eq!(
        (&inbox["parked"], &inbox["unpushed"]),
        (&Value::Null, &json!("board::sync::edited"))
    );
    let written = notes.read(&Notes::note(&inbox));
    let frontmatter = "---\nremote: \"crumbkit/board\"\ncandidates: []\ntopic: \"T\"\n---\n\n";
    assert!(written.starts_with(frontmatter), "{written}");
    let parked = notes
        .call(
            "add",
            &["--text", "Mine.", "--repo", "/repos/notes-personal"],
        )
        .unwrap();
    assert_eq!(parked["parked"], "board::sync::edited");
}

#[test]
fn two_inbox_notes_in_one_second_are_two_files() {
    let notes = Notes::new();
    let _work = notes.another("notes-work");
    notes.cover(&notes.clone, "notes-personal", &[], true);
    let now = jiff::Timestamp::now();
    let taken: Vec<String> = (0..120)
        .map(|second| {
            let at =
                (now + jiff::SignedDuration::from_secs(second)).to_zoned(jiff::tz::TimeZone::UTC);
            format!("inbox/{}.md", at.strftime("%Y-%m-%d-%H%M%S"))
        })
        .collect();
    for path in &taken {
        notes.clone.write(path, Item::File("Earlier.\n"));
    }
    commit(notes.clone.workdir());
    git(notes.clone.workdir(), &["push", "-q"]);
    let output = notes
        .call("add", &["--text", "Later.", "--remote", BOARD_REMOTE])
        .unwrap();
    let path = Notes::note(&output);
    let first = format!("{}.md", path.trim_end_matches("-2.md"));
    assert!(path.ends_with("-2.md") && taken.contains(&first), "{path}");
    for each in &taken {
        assert_eq!(
            notes.read(each),
            "Earlier.\n",
            "no earlier note was touched"
        );
    }
}

#[test]
fn the_boards_own_tree_that_does_not_commit_puts_reports_a_null_commit_and_one_that_does_its_commit()
 {
    let notes = Notes::new();
    let board = notes.board.workdir();
    git(board, &["rm", "-q", "repos/notes-personal.json"]);
    layout(&notes.board);
    commit(board);
    let committed = notes.call("add", &["--text", "Committed."]).unwrap();
    assert_eq!(
        committed["commit"],
        json!(git(board, &["rev-parse", "HEAD"]))
    );
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["notes"]}"#;
    notes.board.write(".board/board.json", Item::File(config));
    commit(board);
    let output = notes.call("add", &["--text", "Uncommitted."]).unwrap();
    assert_eq!(
        (&output["commit"], &output["unpushed"]),
        (&Value::Null, &json!("notes::repo::not_synced"))
    );
    let path = output["path"].as_str().unwrap().trim_start_matches('/');
    let written = std::fs::read_to_string(board.join(path)).unwrap();
    assert!(written.ends_with("Uncommitted.\n"), "{written}");
}
