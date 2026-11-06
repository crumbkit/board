//! `board skills install` and `list` run against a skills directory in a
//! temporary directory, outside any board (docs/plugins.md, Skills).
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the tests run the built board as a child process"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use board_scenario::{Item, Scenario};
use serde_json::Value;

/// One run of `board` in `home`, its exit code and its JSON document.
fn board(home: &Scenario, arguments: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_board"))
        .arg("--json")
        .args(arguments)
        .current_dir(home.workdir())
        .env_clear()
        .env("HOME", home.workdir())
        .env("XDG_CACHE_HOME", home.workdir().join(".cache"))
        .env("XDG_CONFIG_HOME", home.workdir().join(".config"))
        .output()
        .unwrap();
    let document = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap(), document)
}

/// `skills <command> --dir skills`, with `more` after it.
fn skills(home: &Scenario, command: &str, more: &[&str]) -> (i32, Value) {
    let arguments = [&["skills", command, "--dir", "skills"], more].concat();
    board(home, &arguments)
}

/// Each row's name and state.
fn states(document: &Value) -> Vec<(String, String)> {
    let rows = document["data"].as_array().unwrap();
    rows.iter()
        .map(|row| {
            let field = |name: &str| row[name].as_str().unwrap().to_owned();
            (field("name"), field("state"))
        })
        .collect()
}

/// The state of `name` in `document`.
fn state_of(document: &Value, name: &str) -> String {
    let states = states(document);
    let found = states.into_iter().find(|(row, _)| row == name);
    found.unwrap_or_else(|| panic!("no row {name}")).1
}

/// The one row install reported as `backed_up`, its name and its path.
fn backed_up(document: &Value) -> (String, String) {
    let rows = document["data"].as_array().unwrap();
    let mut backups = rows.iter().filter(|row| row["state"] == "backed_up");
    let row = backups
        .next()
        .unwrap_or_else(|| panic!("no backup in {document}"));
    assert!(backups.next().is_none(), "{document}");
    let field = |name: &str| row[name].as_str().unwrap().to_owned();
    (field("name"), field("path"))
}

fn path(home: &Scenario, relative: &str) -> PathBuf {
    home.workdir().join(relative)
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_fresh_install_writes_every_skill_with_its_marker_and_a_second_changes_nothing() {
    let home = Scenario::new();
    let (code, listed) = skills(&home, "list", &[]);
    assert_eq!(code, 0);
    assert_eq!(listed["schema"], "board/skills-list-report/v2");
    let names: Vec<String> = states(&listed).into_iter().map(|(name, _)| name).collect();
    assert!(states(&listed).iter().all(|(_, state)| state == "missing"));
    assert!(
        names.contains(&"changes".to_owned()) && names.contains(&"understanding-check".to_owned())
    );

    let (code, installed) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert_eq!(installed["schema"], "board/skills-install-report/v2");
    assert_eq!(states(&installed).len(), names.len());
    assert!(
        states(&installed)
            .iter()
            .all(|(_, state)| state == "written")
    );
    for name in &names {
        let marker = text(&path(&home, &format!("skills/{name}/.board-skill")));
        let words: Vec<&str> = marker.split_whitespace().collect();
        assert!(
            words.len() == 3 && words[0] == "board" && words[2] == name && marker.ends_with('\n'),
            "{marker}"
        );
        assert!(path(&home, &format!("skills/{name}/SKILL.md")).is_file());
    }
    assert!(path(&home, "skills/changes/README.md").is_file());

    let (code, again) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert!(states(&again).iter().all(|(_, state)| state == "unchanged"));
    let (_, listed) = skills(&home, "list", &[]);
    assert!(states(&listed).iter().all(|(_, state)| state == "current"));
}

#[test]
fn an_edited_skill_is_listed_stale_and_install_restores_it() {
    let home = Scenario::new();
    skills(&home, "install", &[]);
    let skill = path(&home, "skills/evals/SKILL.md");
    let shipped = text(&skill);
    home.write("skills/evals/SKILL.md", Item::File("edited\n"));
    let (_, listed) = skills(&home, "list", &[]);
    assert_eq!(state_of(&listed, "evals"), "stale");
    assert_eq!(state_of(&listed, "changes"), "current");

    let (code, installed) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&installed, "evals"), "updated");
    assert_eq!(state_of(&installed, "changes"), "unchanged");
    assert_eq!(text(&skill), shipped);
    let leftovers = std::fs::read_dir(path(&home, "skills")).unwrap().flatten();
    let hidden: Vec<_> = leftovers
        .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(hidden.is_empty(), "install left {hidden:?}");
}

#[test]
fn list_names_each_state_and_changes_nothing() {
    let home = Scenario::new();
    home.write("skills/changes/notes.md", Item::File("mine\n"))
        .write("skills/evals/.board-skill", Item::File("board 0.0.0\n"))
        .write("skills/evals/SKILL.md", Item::File("old\n"))
        .write("skills/old/.board-skill", Item::File("board 0.0.0 old\n"))
        .write("skills/old/SKILL.md", Item::File("old\n"))
        .write("skills/unrelated/SKILL.md", Item::File("mine\n"));
    let (code, listed) = skills(&home, "list", &[]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&listed, "changes"), "not_board");
    assert_eq!(state_of(&listed, "evals"), "stale");
    assert_eq!(state_of(&listed, "old"), "retired");
    assert_eq!(state_of(&listed, "tasks"), "missing");
    assert!(!states(&listed).iter().any(|(name, _)| name == "unrelated"));
    assert_eq!(text(&path(&home, "skills/evals/SKILL.md")), "old\n");
    assert!(!path(&home, "skills/tasks").exists());

    skills(&home, "install", &["--force"]);
    home.write("skills/tasks", Item::Deleted);
    let (_, listed) = skills(&home, "list", &[]);
    assert_eq!(state_of(&listed, "changes"), "current");
    assert_eq!(state_of(&listed, "tasks"), "missing");
}

#[test]
fn a_folder_that_is_not_boards_is_refused_before_any_write_and_moved_aside_with_force() {
    let home = Scenario::new();
    home.write("skills/changes/notes.md", Item::File("mine\n"));
    let (code, refused) = skills(&home, "install", &[]);
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "board::skills::not_board");
    assert_eq!(refused["fields"]["folders"], serde_json::json!(["changes"]));
    assert_eq!(refused["fields"]["dir"], "skills");
    assert!(refused["recovery"].to_string().contains("--force"));
    assert!(!path(&home, "skills/evals").exists());
    assert!(!path(&home, "skills/changes/SKILL.md").exists());

    let (code, admitted) = skills(&home, "install", &["--force"]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&admitted, "changes"), "written");
    assert!(!path(&home, "skills/changes/notes.md").exists());
    assert!(path(&home, "skills/changes/.board-skill").is_file());
    let (name, shown) = backed_up(&admitted);
    let stamp = name.strip_prefix("changes.board-backup-").unwrap();
    assert!(stamp.len() == 16 && stamp.ends_with('Z'), "{name}");
    assert_eq!(shown, format!("skills/{name}"));
    assert_eq!(
        text(&path(&home, &format!("skills/{name}/notes.md"))),
        "mine\n"
    );

    let (_, listed) = skills(&home, "list", &[]);
    assert!(!states(&listed).iter().any(|(row, _)| *row == name));
    let (code, again) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert!(!states(&again).iter().any(|(row, _)| *row == name));
    assert_eq!(
        text(&path(&home, &format!("skills/{name}/notes.md"))),
        "mine\n"
    );
}

#[test]
fn a_symlink_at_a_skills_name_is_refused_and_with_force_the_link_alone_is_moved_aside() {
    let home = Scenario::new();
    let checkout = Scenario::new();
    checkout.write("SKILL.md", Item::File("from a checkout\n"));
    home.write("skills/changes", Item::Symlink(&checkout.path()));
    let (code, refused) = skills(&home, "install", &[]);
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "board::skills::not_board");

    let (code, admitted) = skills(&home, "install", &["--force"]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&admitted, "changes"), "written");
    let link = std::fs::symlink_metadata(path(&home, "skills/changes")).unwrap();
    assert!(link.is_dir() && !link.file_type().is_symlink());
    assert_eq!(
        text(&checkout.workdir().join("SKILL.md")),
        "from a checkout\n"
    );
    assert!(!checkout.workdir().join(".board-skill").exists());
    assert!(!checkout.workdir().join("README.md").exists());
    let (name, _) = backed_up(&admitted);
    let backup = path(&home, &format!("skills/{name}"));
    assert!(
        std::fs::symlink_metadata(&backup)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&backup).unwrap(),
        Path::new(&checkout.path())
    );
}

#[test]
fn install_removes_a_board_folder_for_a_skill_it_does_not_ship_and_leaves_others_alone() {
    let home = Scenario::new();
    home.write("skills/gone/.board-skill", Item::File("board 0.0.0 gone\n"))
        .write("skills/gone/SKILL.md", Item::File("gone\n"))
        .write("skills/mine/SKILL.md", Item::File("mine\n"))
        .write("skills/stray.md", Item::File("mine\n"));
    let (code, installed) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&installed, "gone"), "removed");
    assert!(!path(&home, "skills/gone").exists());
    assert_eq!(text(&path(&home, "skills/mine/SKILL.md")), "mine\n");
    assert_eq!(text(&path(&home, "skills/stray.md")), "mine\n");
}

#[test]
fn a_copy_of_a_skill_under_another_name_or_a_folder_with_an_unnamed_marker_is_listed_and_kept() {
    let home = Scenario::new();
    skills(&home, "install", &[]);
    let shipped = text(&path(&home, "skills/changes/.board-skill"));
    home.write("skills/my-changes/.board-skill", Item::File(&shipped))
        .write("skills/my-changes/SKILL.md", Item::File("edited\n"))
        .write("skills/older/.board-skill", Item::File("board 0.0.0\n"))
        .write("skills/older/SKILL.md", Item::File("older\n"));
    let (code, listed) = skills(&home, "list", &[]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&listed, "my-changes"), "copy");
    assert_eq!(state_of(&listed, "older"), "copy");

    let (code, installed) = skills(&home, "install", &[]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&installed, "my-changes"), "kept");
    assert_eq!(state_of(&installed, "older"), "kept");
    assert_eq!(text(&path(&home, "skills/my-changes/SKILL.md")), "edited\n");
    assert_eq!(text(&path(&home, "skills/older/SKILL.md")), "older\n");
}

#[test]
fn a_folder_with_an_unnamed_marker_at_a_skills_name_is_boards_and_updated() {
    let home = Scenario::new();
    home.write("skills/changes/.board-skill", Item::File("board 0.0.0\n"))
        .write("skills/changes/SKILL.md", Item::File("old\n"));
    let (code, installed) = skills(&home, "install", &[]);
    assert_eq!(code, 0, "{installed}");
    assert_eq!(state_of(&installed, "changes"), "updated");
    let marker = text(&path(&home, "skills/changes/.board-skill"));
    assert!(marker.trim_end().ends_with(" changes"), "{marker}");
}

#[test]
fn a_folder_whose_marker_names_another_skill_at_a_skills_name_is_not_boards() {
    let home = Scenario::new();
    home.write(
        "skills/changes/.board-skill",
        Item::File("board 0.0.0 evals\n"),
    )
    .write("skills/changes/SKILL.md", Item::File("mine\n"));
    let (_, listed) = skills(&home, "list", &[]);
    assert_eq!(state_of(&listed, "changes"), "not_board");
    let (code, refused) = skills(&home, "install", &[]);
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "board::skills::not_board");
    assert_eq!(text(&path(&home, "skills/changes/SKILL.md")), "mine\n");
}

#[test]
fn a_directory_that_cannot_be_read_is_refused_and_one_that_can_is_admitted() {
    let home = Scenario::new();
    home.write("file", Item::File("not a directory\n"));
    let (code, refused) = board(&home, &["skills", "install", "--dir", "file"]);
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "board::skills::write_failed");
    assert_eq!(refused["fields"]["dir"], "file");
    let (code, _) = board(&home, &["skills", "install", "--dir", "fresh"]);
    assert_eq!(code, 0);
    assert!(path(&home, "fresh/changes/SKILL.md").is_file());
}

#[test]
fn without_dir_install_writes_into_the_claude_skills_directory_in_home() {
    let home = Scenario::new();
    let (code, _) = board(&home, &["skills", "install"]);
    assert_eq!(code, 0);
    assert!(path(&home, ".claude/skills/changes/.board-skill").is_file());
}

#[test]
fn a_folder_named_as_a_skill_in_other_case_is_refused_and_keeps_its_files_and_force_installs() {
    let home = Scenario::new();
    home.write("skills/Changes/notes.md", Item::File("mine\n"));
    let (code, refused) = skills(&home, "install", &[]);
    assert_eq!(code, 1);
    assert_eq!(refused["code"], "board::skills::not_board");
    assert_eq!(text(&path(&home, "skills/Changes/notes.md")), "mine\n");
    assert!(!path(&home, "skills/evals").exists());

    let (code, admitted) = skills(&home, "install", &["--force"]);
    assert_eq!(code, 0);
    assert_eq!(state_of(&admitted, "changes"), "written");
    assert!(path(&home, "skills/changes/.board-skill").is_file());
}

#[test]
fn a_leftover_scratch_folder_without_a_marker_is_cleared_and_never_listed() {
    for scratch in [".changes.board-new", ".changes.board-old"] {
        let home = Scenario::new();
        home.write(&format!("skills/{scratch}/notes.md"), Item::File("left\n"));
        let (_, listed) = skills(&home, "list", &[]);
        assert!(!states(&listed).iter().any(|(name, _)| name == scratch));
        let (code, installed) = skills(&home, "install", &[]);
        assert_eq!(code, 0, "{installed}");
        assert_eq!(state_of(&installed, "changes"), "written");
        assert!(!path(&home, &format!("skills/{scratch}")).exists());
        assert!(path(&home, "skills/changes/.board-skill").is_file());
    }
}

#[test]
fn skills_refuses_c_and_personal_and_admits_the_same_line_without_them() {
    let home = Scenario::new();
    for refused in [
        &["-C", ".", "skills", "list", "--dir", "skills"][..],
        &["--personal", "skills", "list", "--dir", "skills"],
    ] {
        let (code, document) = board(&home, refused);
        assert_eq!(code, 2, "{document}");
        assert_eq!(document["code"], "board::cli::usage");
        assert!(document["recovery"].to_string().contains("--dir"));
    }
    let (code, _) = board(&home, &["skills", "list", "--dir", "skills"]);
    assert_eq!(code, 0);
}

#[test]
fn a_failed_read_says_it_could_not_read() {
    let home = Scenario::new();
    home.write("file", Item::File("not a directory\n"));
    let (_, refused) = board(&home, &["skills", "list", "--dir", "file"]);
    assert_eq!(refused["code"], "board::skills::write_failed");
    let message = refused["message"].as_str().unwrap();
    assert!(message.starts_with("cannot read "), "{message}");
}
