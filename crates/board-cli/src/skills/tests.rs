use std::collections::BTreeSet;
use std::path::Path;

use super::*;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The name in the frontmatter of `skill`, the text of a SKILL.md.
fn frontmatter_name(skill: &str) -> Option<&str> {
    let body = skill.strip_prefix("---\n")?;
    let (frontmatter, _) = body.split_once("\n---")?;
    let line = frontmatter.lines().find(|line| line.starts_with("name:"))?;
    Some(line["name:".len()..].trim().trim_matches(['\'', '"']))
}

/// The names of the directories below `directory`, sorted.
fn subdirectories(directory: &Path) -> Vec<String> {
    let entries = std::fs::read_dir(directory).into_iter().flatten().flatten();
    let mut names: Vec<String> = entries
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn each_embedded_skill_names_itself_in_its_frontmatter_as_its_folder_is_named() {
    assert!(!SKILLS.is_empty());
    for skill in SKILLS {
        let (_, text) = (skill.files.iter())
            .find(|(path, _)| *path == "SKILL.md")
            .unwrap_or_else(|| panic!("{} embeds no SKILL.md", skill.name));
        let text = std::str::from_utf8(text).unwrap();
        assert_eq!(frontmatter_name(text), Some(skill.name), "{}", skill.name);
    }
    let names: BTreeSet<&str> = SKILLS.iter().map(|skill| skill.name).collect();
    assert_eq!(names.len(), SKILLS.len(), "two skills share a name");
}

#[test]
fn every_skill_directory_under_plugins_is_embedded_in_a_build_with_every_feature() {
    let plugins = Path::new(ROOT).join("plugins");
    let mut found = BTreeSet::new();
    for plugin in subdirectories(&plugins) {
        let directory = plugins.join(&plugin);
        if directory.join("SKILL.md").is_file() {
            found.insert(plugin.clone());
        }
        for skill in subdirectories(&directory.join("skills")) {
            if directory
                .join("skills")
                .join(&skill)
                .join("SKILL.md")
                .is_file()
            {
                found.insert(skill);
            }
        }
    }
    let embedded: BTreeSet<String> = SKILLS.iter().map(|skill| skill.name.to_owned()).collect();
    assert_eq!(embedded, found);
    assert!(found.contains("understanding-check") && found.contains("changes"));
}

#[test]
fn a_plugins_own_skill_embeds_its_skill_and_readme_and_a_skills_folder_embeds_its_files() {
    let files = |name: &str| -> Vec<&str> {
        let skill = SKILLS.iter().find(|skill| skill.name == name).unwrap();
        skill.files.iter().map(|(path, _)| *path).collect()
    };
    assert_eq!(files("changes"), ["SKILL.md", "README.md"]);
    assert_eq!(files("understanding-check"), ["SKILL.md"]);
}

#[test]
fn no_claude_code_mod_ships_a_skill() {
    let plugins = Path::new(ROOT).join("plugins");
    let shipping: Vec<String> = subdirectories(&plugins)
        .into_iter()
        .filter(|plugin| plugins.join(plugin).join("claude-code/skills").exists())
        .collect();
    assert!(
        shipping.is_empty(),
        "these mods ship skills, which belong in plugins/<plugin>/skills/: {shipping:?}"
    );
}

const ONE: Skill = Skill {
    name: "one",
    files: &[("SKILL.md", b"one")],
};
const TWO: Skill = Skill {
    name: "two",
    files: &[("SKILL.md", b"two")],
};
const THREE: Skill = Skill {
    name: "three",
    files: &[("SKILL.md", b"three")],
};

/// A Board folder whose marker names `skill`, holding `files`.
fn marked(skill: Option<&str>, files: &[(&str, &[u8])]) -> Held {
    let files = files.iter();
    Held::Board {
        skill: skill.map(str::to_owned),
        files: files
            .map(|(path, bytes)| ((*path).to_owned(), Some(bytes.to_vec())))
            .collect(),
    }
}

/// A Board folder whose marker names no skill, holding `files`.
fn board(files: &[(&str, &[u8])]) -> Held {
    marked(None, files)
}

#[test]
fn a_folder_is_current_stale_missing_not_boards_or_retired_by_what_it_holds() {
    let held = BTreeMap::from([
        ("one".to_owned(), board(&[("SKILL.md", b"one")])),
        ("two".to_owned(), board(&[("SKILL.md", b"old")])),
        ("three".to_owned(), Held::Foreign),
        (
            "gone".to_owned(),
            marked(Some("gone"), &[("SKILL.md", b"gone")]),
        ),
        ("mine".to_owned(), Held::Foreign),
    ]);
    let four = Skill {
        name: "four",
        files: &[("SKILL.md", b"four")],
    };
    assert_eq!(
        listed(&[ONE, TWO, THREE, four], &held),
        [
            ("four".to_owned(), Listed::Missing),
            ("gone".to_owned(), Listed::Retired),
            ("one".to_owned(), Listed::Current),
            ("three".to_owned(), Listed::NotBoard),
            ("two".to_owned(), Listed::Stale),
        ]
    );
}

#[test]
fn only_a_marker_naming_its_own_folder_retires_it_and_a_marker_naming_another_skill_is_not_boards()
{
    let held = BTreeMap::from([
        (
            "copy".to_owned(),
            marked(Some("one"), &[("SKILL.md", b"x")]),
        ),
        ("older".to_owned(), board(&[("SKILL.md", b"x")])),
        (
            "one".to_owned(),
            marked(Some("two"), &[("SKILL.md", b"one")]),
        ),
        (
            "two".to_owned(),
            marked(Some("two"), &[("SKILL.md", b"two")]),
        ),
    ]);
    assert_eq!(
        listed(&[ONE, TWO], &held),
        [
            ("copy".to_owned(), Listed::Copy),
            ("older".to_owned(), Listed::Copy),
            ("one".to_owned(), Listed::NotBoard),
            ("two".to_owned(), Listed::Current),
        ]
    );
}

#[test]
fn a_markers_skill_is_its_third_word() {
    assert_eq!(
        named("board 1.2.3+abc changes\n").as_deref(),
        Some("changes")
    );
    assert_eq!(named("board 1.2.3\n"), None);
}

#[test]
fn a_board_folder_with_an_extra_file_or_an_entry_that_is_no_file_is_stale() {
    let extra = board(&[("SKILL.md", b"one"), ("notes.md", b"mine")]);
    let mut odd = BTreeMap::from([("SKILL.md".to_owned(), Some(b"one".to_vec()))]);
    odd.insert("link".to_owned(), None);
    let odd = Held::Board {
        skill: None,
        files: odd,
    };
    for held in [extra, odd] {
        let held = BTreeMap::from([("one".to_owned(), held)]);
        assert_eq!(listed(&[ONE], &held), [("one".to_owned(), Listed::Stale)]);
    }
}

#[test]
fn a_folder_named_as_a_skill_in_other_case_is_read_as_that_skills_folder() {
    let raw = BTreeMap::from([
        ("Changes".to_owned(), Held::Foreign),
        ("Mine".to_owned(), Held::Foreign),
    ]);
    let changes = Skill {
        name: "changes",
        files: &[("SKILL.md", b"x")],
    };
    let held = keyed(raw, &[changes, ONE]);
    assert_eq!(held.get("changes"), Some(&Held::Foreign));
    assert!(held.contains_key("Mine") && !held.contains_key("Changes"));
    let exact = BTreeMap::from([
        ("one".to_owned(), board(&[("SKILL.md", b"one")])),
        ("ONE".to_owned(), Held::Foreign),
    ]);
    let held = keyed(exact, &[ONE]);
    assert_eq!(held.get("one"), Some(&board(&[("SKILL.md", b"one")])));
}

#[test]
fn a_backup_takes_the_stamped_name_or_the_first_free_one_after_it() {
    let home = board_scenario::Scenario::new();
    let dir = home.workdir();
    let stamp = "20261106T101500Z";
    let first = backup(dir, "changes", stamp);
    assert_eq!(first, dir.join("changes.board-backup-20261106T101500Z"));
    home.write(
        "changes.board-backup-20261106T101500Z/notes.md",
        board_scenario::Item::File("x\n"),
    )
    .write(
        "changes.board-backup-20261106T101500Z-2",
        board_scenario::Item::File("x\n"),
    );
    assert_eq!(
        backup(dir, "changes", stamp),
        dir.join("changes.board-backup-20261106T101500Z-3")
    );
}

#[test]
fn a_hint_quotes_a_directory_a_shell_would_split() {
    assert_eq!(quote(Path::new("skills")), "skills");
    assert_eq!(quote(Path::new("my skills")), "'my skills'");
    assert_eq!(quote(Path::new("it's")), r"'it'\''s'");
}
