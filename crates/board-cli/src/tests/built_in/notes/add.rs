//! `notes.add` into a notes repo with a remote and a phone that pushes to
//! it. These were `test_add.py`.

use super::*;

/// Whether `path` is a daily note's path.
fn daily(path: &str) -> bool {
    let Some(date) = path
        .strip_prefix("daily/")
        .and_then(|rest| rest.strip_suffix(".md"))
    else {
        return false;
    };
    date.len() == 10 && date.parse::<jiff::civil::Date>().is_ok()
}

#[test]
fn a_note_lands_in_todays_daily_note_under_a_time_heading_and_reaches_the_remote() {
    let notes = Notes::new();
    notes
        .clone
        .write(".reflect/index.sqlite", Item::File("Reflect's own index"));
    let output = notes.call("add", &["--text", "Call the bank."]).unwrap();
    let path = Notes::note(&output);
    assert!(daily(&path), "{path}");
    let head = notes.head();
    assert_eq!(
        output,
        json!({
            "repo": "/repos/notes-personal",
            "path": format!("/repos/notes-personal/{path}"),
            "commit": head,
            "candidates": null,
            "parked": null,
            "unpushed": null,
        })
    );
    assert!(section(&notes.read(&path), "Call the bank.\n"));
    let remote = notes.beside.path().join("remote.git");
    assert_eq!(git(&remote, &["rev-parse", "main"]), head);
    assert_eq!(notes.pin(), head);
    let changed = git(
        notes.clone.workdir(),
        &["show", "--name-only", "--format=", &head],
    );
    assert_eq!(changed, path, "only the note is committed");
    let tracked = git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(!tracked.contains(".reflect"), "{tracked}");
}

#[test]
fn the_daily_note_is_the_one_for_the_date_in_the_callers_time_zone() {
    let notes = Notes::new();
    for zone in ZONES {
        let output = notes
            .call_in("add", &["--text", "Here."], zone, None)
            .unwrap();
        let expected: Vec<String> = (dates(zone, 0).iter())
            .map(|date| format!("daily/{date}.md"))
            .collect();
        assert!(expected.contains(&Notes::note(&output)), "{output}");
    }
}

#[test]
fn a_second_note_is_appended_as_a_new_section_after_what_the_phone_pushed() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes.phone_appends(&path, "\nFrom the phone.\n");
    let output = notes.call("add", &["--text", "Second."]).unwrap();
    assert_eq!(Notes::note(&output), path);
    let text = notes.remote_text(&path);
    let sections: Vec<&str> = text.split("\n\n## ").collect();
    assert_eq!(sections.len(), 2, "{text}");
    assert!(
        sections[0].ends_with("\n\nFirst.\n\nFrom the phone."),
        "{text}"
    );
    assert!(sections[1].ends_with("\n\nSecond."), "{text}");
}

#[test]
fn text_keeps_its_leading_indentation_and_text_of_only_whitespace_is_refused() {
    let notes = Notes::new();
    let refused = notes.refused("add", &["--text", " \n\t "]);
    assert_eq!(refused["code"], "board::plugin::bad_input");
    let output = notes
        .call("add", &["--text", "    indented code\n\n"])
        .unwrap();
    assert!(section(
        &notes.read(&Notes::note(&output)),
        "    indented code\n"
    ));
}

#[test]
fn a_commit_the_desktop_app_made_and_did_not_push_is_kept_and_pushed_with_the_note() {
    let notes = Notes::new();
    notes
        .clone
        .write("notes/groceries.md", Item::File("# Groceries\n"));
    let desktop = commit(notes.clone.workdir());
    notes.call("add", &["--text", "Buy milk."]).unwrap();
    let remote = notes.beside.path().join("remote.git");
    git(&remote, &["merge-base", "--is-ancestor", &desktop, "main"]);
    assert_eq!(notes.remote_text("notes/groceries.md"), "# Groceries");
}

#[test]
fn a_topic_note_is_started_with_its_title_at_reflects_slug_and_then_appended_to() {
    let notes = Notes::new();
    notes
        .call("add", &["--text", "Agenda.", "--topic", "Meeting Notes!"])
        .unwrap();
    let output = notes
        .call("add", &["--text", "Minutes.", "--topic", "Meeting Notes!"])
        .unwrap();
    assert_eq!(
        output["path"],
        "/repos/notes-personal/notes/meeting-notes.md"
    );
    let text = notes.remote_text("notes/meeting-notes.md");
    let (title, rest) = text.split_once("\n\n").unwrap();
    assert_eq!(title, "# Meeting Notes!");
    let (agenda, minutes) = rest.split_once("\n\n## ").unwrap();
    assert!(section(&format!("{agenda}\n"), "Agenda.\n"), "{text}");
    assert!(section(&format!("## {minutes}\n"), "Minutes.\n"), "{text}");
}

#[test]
fn a_note_that_is_not_utf8_is_refused_and_left_as_it_was_and_one_that_is_is_written() {
    let notes = Notes::new();
    notes
        .phone
        .write("notes/binary.md", Item::Bytes(b"\xff\xfe not text\n"));
    commit(notes.phone.workdir());
    git(notes.phone.workdir(), &["push", "-q"]);
    let refused = notes.refused("add", &["--text", "More.", "--topic", "Binary"]);
    assert_eq!(refused["code"], "notes::note::not_text");
    assert_eq!(
        refused["fields"]["path"],
        "/repos/notes-personal/notes/binary.md"
    );
    let bytes = std::fs::read(notes.clone.workdir().join("notes/binary.md")).unwrap();
    assert_eq!(bytes, b"\xff\xfe not text\n");
    notes
        .call("add", &["--text", "More.", "--topic", "Text"])
        .unwrap();
}

#[test]
fn a_daily_note_with_an_edit_the_desktop_app_has_not_committed_parks_the_note_beside_it() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes
        .clone
        .write(&path, Item::Appended("\nTyping in Reflect"));
    let output = notes.call("add", &["--text", "Second."]).unwrap();
    assert_eq!(
        (&output["path"], &output["parked"]),
        (
            &json!(format!("/repos/notes-personal/{}", sibling(&path))),
            &json!("board::mount::stale")
        )
    );
    assert!(section(&notes.remote_text(&sibling(&path)), "Second."));
    assert!(
        notes.read(&path).ends_with("Typing in Reflect"),
        "the edit is left"
    );
    let status = git(notes.clone.workdir(), &["status", "--porcelain"]);
    assert_eq!(status, format!("M {path}"));
}

#[test]
fn a_daily_note_without_an_uncommitted_edit_is_not_parked() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes
        .clone
        .write(&path, Item::Appended("\nTyped in Reflect\n"));
    commit(notes.clone.workdir());
    let output = notes.call("add", &["--text", "Second."]).unwrap();
    assert_eq!(
        (&output["path"], &output["parked"]),
        (
            &json!(format!("/repos/notes-personal/{path}")),
            &Value::Null
        )
    );
}

#[test]
fn a_note_and_its_sibling_both_holding_uncommitted_edits_are_busy_and_nothing_is_written() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes
        .clone
        .write(&path, Item::Appended("\nTyping in Reflect"));
    notes.call("add", &["--text", "Parked."]).unwrap();
    notes
        .clone
        .write(&sibling(&path), Item::Appended("\nMoving it"));
    let head = notes.head();
    let refused = notes.refused("add", &["--text", "Third."]);
    assert_eq!(refused["code"], "notes::note::busy");
    assert!(
        refused["message"].as_str().unwrap().contains("call again"),
        "{refused}"
    );
    assert_eq!(
        refused["fields"]["path"],
        format!("/repos/notes-personal/{}", sibling(&path))
    );
    assert_eq!(notes.head(), head);
    let both = notes.read(&path) + &notes.read(&sibling(&path));
    assert!(!both.contains("Third."), "{both}");
}

#[test]
fn a_desktop_commit_beside_a_phone_push_to_another_note_syncs_and_is_not_parked() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes
        .clone
        .write(&path, Item::Appended("\nFrom the desktop.\n"));
    commit(notes.clone.workdir());
    notes.phone_appends("notes/phone.md", "From the phone.\n");
    let output = notes.call("add", &["--text", "From the agent."]).unwrap();
    assert_eq!(
        (&output["path"], &output["parked"], &output["unpushed"]),
        (
            &json!(format!("/repos/notes-personal/{path}")),
            &Value::Null,
            &Value::Null
        )
    );
}

#[test]
fn a_desktop_commit_and_a_phone_push_to_one_note_merge_with_a_recorded_conflict_and_the_note_is_not_parked()
 {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes
        .clone
        .write(&path, Item::Appended("\nFrom the desktop.\n"));
    commit(notes.clone.workdir());
    notes.phone_appends(&path, "\nFrom the phone.\n");
    let output = notes.call("add", &["--text", "From the agent."]).unwrap();
    assert_eq!(output["parked"], Value::Null);
    assert_eq!(output["unpushed"], Value::Null);
    assert_eq!(output["path"], format!("/repos/notes-personal/{path}"));
    assert_eq!(output["commit"], json!(notes.head()));
    assert_eq!(notes.pin(), notes.head());
    let kept = notes.read(&path);
    assert!(kept.contains("From the phone."), "{kept}");
    assert!(!kept.contains("From the desktop."), "{kept}");
    assert_eq!(git(notes.clone.workdir(), &["status", "--porcelain"]), "");
    let branch = git(notes.clone.workdir(), &["symbolic-ref", "--short", "HEAD"]);
    assert_eq!(branch, "main");
    let gitdir = notes.clone.workdir().join(".git");
    for name in ["MERGE_HEAD", "REBASE_HEAD", "rebase-merge", "rebase-apply"] {
        assert!(!gitdir.join(name).exists(), "{name}");
    }
}

#[test]
fn a_desktop_commit_and_a_phone_push_to_different_parts_of_one_note_both_survive() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes.call("add", &["--text", "Second."]).unwrap();
    let kept = notes.read(&path);
    notes
        .clone
        .write(&path, Item::File(&format!("From the desktop.\n{kept}")));
    commit(notes.clone.workdir());
    notes.phone_appends(&path, "\nFrom the phone.\n");
    let output = notes.call("add", &["--text", "From the agent."]).unwrap();
    assert_eq!(output["parked"], Value::Null);
    assert_eq!(output["unpushed"], Value::Null);
    let merged = notes.read(&path);
    for text in ["From the desktop.", "From the phone.", "From the agent."] {
        assert!(merged.contains(text), "{text}: {merged}");
    }
    assert_eq!(git(notes.clone.workdir(), &["status", "--porcelain"]), "");
}

#[test]
fn a_phone_push_over_an_uncommitted_edit_parks_the_note_and_leaves_the_edit() {
    let notes = Notes::new();
    let path = Notes::note(&notes.call("add", &["--text", "First."]).unwrap());
    notes.phone_appends(&path, "\nFrom the phone.\n");
    notes
        .clone
        .write(&path, Item::Appended("\nTyping in Reflect"));
    let typing = notes.read(&path);
    let output = notes.call("add", &["--text", "From the agent."]).unwrap();
    assert_eq!(
        (&output["path"], &output["parked"], &output["unpushed"]),
        (
            &json!(format!("/repos/notes-personal/{}", sibling(&path))),
            &json!("board::sync::edited"),
            &json!("board::sync::edited")
        )
    );
    assert_eq!(notes.read(&path), typing);
    assert!(section(&notes.read(&sibling(&path)), "From the agent.\n"));
}

#[test]
fn a_push_refused_after_the_note_is_committed_is_reported_and_the_note_is_kept_once() {
    let notes = Notes::new();
    let hooks = Scenario::new();
    hooks.write("pre-receive", Item::Executable("#!/bin/sh\nexit 1\n"));
    let remote = notes.beside.path().join("remote.git");
    git(&remote, &["config", "core.hooksPath", &hooks.path()]);
    let output = notes.call("add", &["--text", "Kept here."]).unwrap();
    assert_eq!(
        (&output["parked"], &output["unpushed"]),
        (&Value::Null, &json!("board::sync::gave_up"))
    );
    assert_eq!(output["commit"], json!(notes.head()));
    assert_eq!(
        notes
            .read(&Notes::note(&output))
            .matches("Kept here.")
            .count(),
        1
    );
}

/// A sync that holds `sync.lock` past the add's five-second wait leaves the
/// note committed, parked in its sibling file because the clone could not be
/// brought up to date, and unpushed; the twin above waits it out and pushes.
#[test]
fn a_note_added_while_a_sync_outlasts_the_wait_is_committed_and_reported_unpushed() {
    let notes = Notes::new();
    let held = notes.clone.flocked(".git/sync.lock");
    let output = notes.call("add", &["--text", "Waited."]).unwrap();
    assert_eq!(
        (&output["parked"], &output["unpushed"]),
        (&json!("board::sync::busy"), &json!("board::sync::busy"))
    );
    assert_eq!(output["commit"], json!(notes.head()));
    assert_eq!(
        notes.read(&Notes::note(&output)).matches("Waited.").count(),
        1
    );
    drop(held);
    let output = notes.call("add", &["--text", "Pushed."]).unwrap();
    assert_eq!(output["unpushed"], Value::Null);
}

#[test]
fn notes_added_at_once_each_land_exactly_once() {
    let notes = Notes::new();
    let texts: Vec<String> = (0..4).map(|n| format!("Note {n}.")).collect();
    let results: Vec<Result<Value, Value>> = std::thread::scope(|scope| {
        let running: Vec<_> = (texts.iter())
            .map(|text| {
                let runner = notes.runner.clone();
                scope.spawn(move || runner.call("add", &["--text", text, "--topic", "Race"]))
            })
            .collect();
        running.into_iter().map(|one| one.join().unwrap()).collect()
    });
    for result in &results {
        assert!(result.is_ok(), "{results:?}");
    }
    let written: String = ["notes/race.md", "notes/race.agent.md"]
        .iter()
        .filter_map(|path| std::fs::read_to_string(notes.clone.workdir().join(path)).ok())
        .collect();
    for text in &texts {
        assert_eq!(written.matches(text.as_str()).count(), 1, "{written}");
    }
}
