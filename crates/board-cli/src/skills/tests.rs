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
fn every_embedded_skill_names_the_plugin_directory_it_was_found_in() {
    let plugins = Path::new(ROOT).join("plugins");
    for skill in SKILLS {
        let directory = plugins.join(skill.plugin);
        let found = if skill.name == skill.plugin {
            directory.join("SKILL.md")
        } else {
            directory.join("skills").join(skill.name).join("SKILL.md")
        };
        assert!(found.is_file(), "{} in plugin {}", skill.name, skill.plugin);
    }
    let changes = SKILLS.iter().find(|skill| skill.name == "changes").unwrap();
    assert_eq!(changes.plugin, "changes");
    let check = SKILLS
        .iter()
        .find(|skill| skill.name == "understanding-check");
    assert_eq!(check.unwrap().plugin, "changes");
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

/// The most characters a skill's description may hold. Claude Code and
/// Codex list every installed skill's description in every session's
/// context, so a long one costs every session; the commands belong in the
/// skill's body, which loads only when the skill is called.
const MAX_DESCRIPTION_CHARS: usize = 200;

/// The description in the frontmatter of `skill`, the text of a SKILL.md,
/// with YAML's single quotes undone.
fn frontmatter_description(skill: &str) -> Option<String> {
    let body = skill.strip_prefix("---\n")?;
    let (frontmatter, _) = body.split_once("\n---")?;
    let line = frontmatter
        .lines()
        .find_map(|line| line.strip_prefix("description:"))?;
    let line = line.trim();
    Some(
        match line
            .strip_prefix('\'')
            .and_then(|line| line.strip_suffix('\''))
        {
            Some(quoted) => quoted.replace("''", "'"),
            None => line.trim_matches('"').to_owned(),
        },
    )
}

/// What is wrong with the description of skill `name`, or `None` when it
/// fits the listing and says when to use the skill.
fn description_fault(name: &str, description: &str) -> Option<String> {
    let length = description.chars().count();
    if length > MAX_DESCRIPTION_CHARS {
        return Some(format!(
            "{name}'s description holds {length} characters, more than {MAX_DESCRIPTION_CHARS}"
        ));
    }
    if description.ends_with("say more.") {
        return Some(format!(
            "{name}'s description ends by pointing at other files"
        ));
    }
    if !description.contains("Use it") {
        return Some(format!("{name}'s description does not say when to use it"));
    }
    None
}

#[test]
fn every_embedded_skills_description_fits_in_200_characters_and_says_when_to_use_it() {
    let faults: Vec<String> = SKILLS
        .iter()
        .filter_map(|skill| {
            let (_, text) = skill.files.iter().find(|(path, _)| *path == "SKILL.md")?;
            let text = std::str::from_utf8(text).unwrap();
            let description = frontmatter_description(text)
                .unwrap_or_else(|| panic!("{} has no description", skill.name));
            description_fault(skill.name, &description)
        })
        .collect();
    assert!(faults.is_empty(), "{faults:#?}");
}

#[test]
fn a_description_of_200_characters_fits_and_one_of_201_does_not() {
    let fits = format!(
        "Use it {}",
        "a".repeat(MAX_DESCRIPTION_CHARS - "Use it ".len())
    );
    assert_eq!(fits.chars().count(), 200);
    assert_eq!(description_fault("one", &fits), None);
    let over = format!("{fits}a");
    assert_eq!(
        description_fault("one", &over).as_deref(),
        Some("one's description holds 201 characters, more than 200")
    );
    let quoted = "---\nname: one\ndescription: 'It''s short. Use it now.'\n---\nbody";
    assert_eq!(
        frontmatter_description(quoted).as_deref(),
        Some("It's short. Use it now.")
    );
    let pointing = "Use it. SKILL.md and README.md in the plugin's directory say more.";
    assert!(description_fault("one", pointing).is_some());
}

const ONE: Skill = Skill {
    name: "one",
    plugin: "a",
    files: &[("SKILL.md", b"one")],
};
const TWO: Skill = Skill {
    name: "two",
    plugin: "a",
    files: &[("SKILL.md", b"two")],
};
const THREE: Skill = Skill {
    name: "three",
    plugin: "b",
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
        (
            "one".to_owned(),
            marked(Some("one"), &[("SKILL.md", b"one")]),
        ),
        (
            "two".to_owned(),
            marked(Some("two"), &[("SKILL.md", b"old")]),
        ),
        ("three".to_owned(), Held::Foreign),
        (
            "gone".to_owned(),
            marked(Some("gone"), &[("SKILL.md", b"gone")]),
        ),
        ("mine".to_owned(), Held::Foreign),
    ]);
    let four = Skill {
        name: "four",
        plugin: "b",
        files: &[("SKILL.md", b"four")],
    };
    assert_eq!(
        listed(&[ONE, TWO, THREE, four], &held, None),
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
fn only_its_own_name_in_a_marker_retires_a_folder_or_makes_one_at_a_skills_name_boards() {
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
        ("three".to_owned(), board(&[("SKILL.md", b"three")])),
    ]);
    assert_eq!(
        listed(&[ONE, TWO, THREE], &held, None),
        [
            ("copy".to_owned(), Listed::Copy),
            ("older".to_owned(), Listed::Copy),
            ("one".to_owned(), Listed::NotBoard),
            ("three".to_owned(), Listed::NotBoard),
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
    let extra = marked(Some("one"), &[("SKILL.md", b"one"), ("notes.md", b"mine")]);
    let mut odd = BTreeMap::from([("SKILL.md".to_owned(), Some(b"one".to_vec()))]);
    odd.insert("link".to_owned(), None);
    let odd = Held::Board {
        skill: Some("one".to_owned()),
        files: odd,
    };
    for held in [extra, odd] {
        let held = BTreeMap::from([("one".to_owned(), held)]);
        assert_eq!(
            listed(&[ONE], &held, None),
            [("one".to_owned(), Listed::Stale)]
        );
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
        plugin: "changes",
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
fn a_backup_takes_the_stamped_name_in_the_hidden_backups_folder_or_the_first_free_one_after_it() {
    let home = board_scenario::Scenario::new();
    let dir = home.workdir();
    let stamp = "20261106T101500Z";
    let first = backup(dir, "changes", stamp);
    assert_eq!(first, dir.join(".board-backups/changes-20261106T101500Z"));
    home.write(
        ".board-backups/changes-20261106T101500Z/notes.md",
        board_scenario::Item::File("x\n"),
    )
    .write(
        ".board-backups/changes-20261106T101500Z-2",
        board_scenario::Item::File("x\n"),
    );
    assert_eq!(
        backup(dir, "changes", stamp),
        dir.join(".board-backups/changes-20261106T101500Z-3")
    );
}

#[test]
fn a_failed_move_names_the_backup_it_could_not_move_back_and_one_moved_back_names_none() {
    let failure = |stranded: Option<PathBuf>| SkillsError::WriteFailed {
        path: PathBuf::from("skills/.changes.board-new"),
        act: Act::Move(PathBuf::from("skills/changes")),
        source: io::Error::other("disk full"),
        stranded,
    };
    let backup = PathBuf::from("skills/.board-backups/changes-20261106T101500Z");
    let refusal = failure(Some(backup)).refusal(Path::new("skills"));
    assert_eq!(refusal.code.as_str(), "board::skills::write_failed");
    assert!(
        refusal
            .message
            .ends_with("is at skills/.board-backups/changes-20261106T101500Z"),
        "{}",
        refusal.message
    );
    assert_eq!(
        refusal.fields["backup"],
        "skills/.board-backups/changes-20261106T101500Z"
    );
    let refusal = failure(None).refusal(Path::new("skills"));
    assert!(!refusal.message.contains("board-backups"));
    assert!(!refusal.fields.contains_key("backup"));
}

#[test]
fn a_hint_quotes_a_directory_a_shell_would_split() {
    assert_eq!(quote(Path::new("skills")), "skills");
    assert_eq!(quote(Path::new("my skills")), "'my skills'");
    assert_eq!(quote(Path::new("it's")), r"'it'\''s'");
}

/// Board folders for `one`, of plugin `a`, and `three`, of plugin `b`,
/// each stale, and a retired folder for `gone`, which no plugin ships.
fn held_by_two_plugins() -> BTreeMap<String, Held> {
    BTreeMap::from([
        (
            "one".to_owned(),
            marked(Some("one"), &[("SKILL.md", b"old")]),
        ),
        (
            "three".to_owned(),
            marked(Some("three"), &[("SKILL.md", b"old")]),
        ),
        (
            "gone".to_owned(),
            marked(Some("gone"), &[("SKILL.md", b"gone")]),
        ),
    ])
}

#[test]
fn a_selection_of_plugins_lists_only_their_skills_and_never_retires_another_plugins_folder() {
    let held = held_by_two_plugins();
    let only = BTreeSet::from(["a".to_owned()]);
    assert_eq!(
        listed(&[ONE, TWO, THREE], &held, Some(&only)),
        [
            ("gone".to_owned(), Listed::Retired),
            ("one".to_owned(), Listed::Stale),
            ("two".to_owned(), Listed::Missing),
        ]
    );
    assert_eq!(
        listed(&[ONE, TWO, THREE], &held, None),
        [
            ("gone".to_owned(), Listed::Retired),
            ("one".to_owned(), Listed::Stale),
            ("three".to_owned(), Listed::Stale),
            ("two".to_owned(), Listed::Missing),
        ]
    );
}

#[test]
fn a_selection_leaves_another_plugins_foreign_folder_unrefused() {
    let held = BTreeMap::from([("three".to_owned(), Held::Foreign)]);
    let only = BTreeSet::from(["a".to_owned()]);
    assert_eq!(
        listed(&[ONE, THREE], &held, Some(&only)),
        [("one".to_owned(), Listed::Missing)]
    );
}

#[test]
fn install_with_a_selection_writes_its_plugins_skills_and_leaves_another_plugins_folder_as_it_was()
{
    let home = board_scenario::Scenario::new();
    let dir = home.workdir().join("skills");
    home.write(
        "skills/three/.board-skill",
        board_scenario::Item::File("board 0.1.0 three\n"),
    )
    .write("skills/three/SKILL.md", board_scenario::Item::File("old"));
    let held = read(&dir).unwrap();
    let only = BTreeSet::from(["a".to_owned()]);
    let done = install(&dir, &[ONE, THREE], &held, false, Some(&only)).unwrap();
    assert_eq!(done, [("one".to_owned(), Installed::Written)]);
    assert_eq!(std::fs::read(dir.join("one/SKILL.md")).unwrap(), b"one");
    assert_eq!(std::fs::read(dir.join("three/SKILL.md")).unwrap(), b"old");
    assert_eq!(
        std::fs::read(dir.join("three/.board-skill")).unwrap(),
        b"board 0.1.0 three\n"
    );
}

/// Each of `names` as an owned string.
fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

#[test]
fn plugins_must_each_be_known_and_name_at_least_one_and_one_with_no_skill_selects_none() {
    let known: BTreeSet<String> = strings(&["a", "b", "c"]).into_iter().collect();
    let only = chosen(&strings(&["a", "c"]), &known).unwrap();
    let expected: BTreeSet<String> = strings(&["a", "c"]).into_iter().collect();
    assert_eq!(only, expected);
    assert_eq!(
        listed(&[ONE, THREE], &BTreeMap::new(), Some(&only)),
        [("one".to_owned(), Listed::Missing)]
    );
    let refusal = chosen(&strings(&["a", "nosuch", "other"]), &known).unwrap_err();
    assert_eq!(refusal.code.as_str(), "board::cli::usage");
    assert!(
        refusal.message.contains("nosuch, other"),
        "{}",
        refusal.message
    );
    let hint = refusal.hint.as_deref().unwrap_or_default();
    assert!(hint.contains("a, b, c"), "{hint}");
    for empty in [&[""][..], &[][..], &[" ", ""][..]] {
        let refusal = chosen(&strings(empty), &known).unwrap_err();
        assert_eq!(refusal.code.as_str(), "board::cli::usage");
    }
}

/// A skill names its plugin by the directory that ships it, which may be a
/// built-in plugin's former name, as plugins/attention/ ships the orders
/// plugin's skill until Board drops the name attention.
#[test]
fn every_embedded_skills_plugin_is_built_in() {
    let built_in: BTreeSet<String> = (board_plugins::BUILT_IN.0.iter())
        .flat_map(|plugin| {
            let manifest = plugin.manifest();
            let former = manifest
                .former_names
                .iter()
                .map(|name| name.as_str().to_owned());
            std::iter::once(manifest.name.as_str().to_owned())
                .chain(former)
                .collect::<Vec<_>>()
        })
        .collect();
    for skill in SKILLS {
        assert!(
            built_in.contains(skill.plugin),
            "{} ships {}",
            skill.plugin,
            skill.name
        );
    }
}
