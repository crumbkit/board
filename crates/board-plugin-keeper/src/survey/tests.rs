//! A synthetic workspace: members by glob, an excluded directory, a renamed
//! dependency and a dependency that is not a member.

use super::*;

fn files(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut files: BTreeMap<String, String> = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\", \"tools/one\"]\nexclude = [\"crates/skipped\"]\n",
        ),
        (
            "crates/core/Cargo.toml",
            "[package]\nname = \"demo-core\"\n[dependencies]\nserde = \"1\"\nbase = { package = \"demo-base\", path = \"../base\" }\n",
        ),
        ("crates/core/src/lib.rs", "pub fn run_all() {}\npub(crate) struct Plan;\n"),
        ("crates/core/src/run.rs", "one\ntwo\nthree"),
        ("crates/core/src/run/part.rs", "x\n"),
        ("crates/core/tests/it.rs", "t\n"),
        (
            "crates/base/Cargo.toml",
            "[package]\nname = \"demo-base\"\n[dev-dependencies]\ndemo-core = { path = \"../core\" }\n",
        ),
        ("crates/base/src/main.rs", "m\n"),
        ("crates/skipped/Cargo.toml", "[package]\nname = \"demo-skipped\"\n"),
        ("crates/skipped/src/lib.rs", "s\n"),
        ("tools/one/Cargo.toml", "[package]\nname = \"demo-tool\"\n[dependencies]\ndemo-base = \"*\"\n"),
        ("tools/one/src/lib.rs", ""),
        ("tools/two/Cargo.toml", "[package]\nname = \"demo-other\"\n"),
    ]
    .into_iter()
    .map(|(path, text)| (path.to_owned(), text.to_owned()))
    .collect();
    files.extend(
        extra
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned())),
    );
    files
}

#[test]
fn members_come_from_the_workspace_globs_less_the_excluded_with_their_lines_and_modules() {
    let found = crates(&files(&[])).unwrap();
    let ids: Vec<&str> = found.iter().map(|each| each.id.as_str()).collect();
    assert_eq!(ids, ["demo-base", "demo-core", "demo-tool"]);
    let core = &found[1];
    assert_eq!(
        core.lines,
        2 + 3 + 1 + 1,
        "every .rs under the crate, a last line with no newline counts"
    );
    let modules: Vec<(&str, u64)> = core
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.lines))
        .collect();
    assert_eq!(
        modules,
        [("lib", 2), ("run", 4)],
        "tests/ is in the crate's lines and in no module"
    );
}

#[test]
fn dependencies_are_the_members_a_crate_names_by_package_in_any_table_and_no_other() {
    let found = crates(&files(&[])).unwrap();
    assert_eq!(found[0].deps, ["demo-core"], "a dev-dependency counts");
    assert_eq!(
        found[1].deps,
        ["demo-base"],
        "a renamed dependency counts; serde is no member"
    );
    assert_eq!(found[2].deps, ["demo-base"]);
}

#[test]
fn a_mount_with_no_workspace_or_a_broken_manifest_says_why_and_a_whole_one_is_read() {
    let none = BTreeMap::new();
    assert!(crates(&none).unwrap_err().contains("no Cargo.toml"));
    let single = files(&[("Cargo.toml", "[package]\nname = \"solo\"\n")]);
    assert!(crates(&single).unwrap_err().contains("no [workspace]"));
    let broken = files(&[("crates/core/Cargo.toml", "[package\n")]);
    assert!(
        crates(&broken)
            .unwrap_err()
            .contains("crates/core/Cargo.toml does not parse")
    );
    crates(&files(&[])).unwrap();
}

#[test]
fn a_root_package_is_a_crate_and_owns_the_files_no_member_does() {
    let rooted = files(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/core\"]\n[package]\nname = \"root\"\n",
        ),
        ("src/lib.rs", "r\nr\n"),
        ("src/extra/mod.rs", "e\n"),
    ]);
    let found = crates(&rooted).unwrap();
    let root = found.iter().find(|each| each.id == "root").unwrap();
    assert_eq!(root.lines, 3);
    let modules: Vec<(&str, u64)> = root
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.lines))
        .collect();
    assert_eq!(modules, [("extra", 1), ("lib", 2)]);
    assert!(
        found.iter().all(|each| each.id != "demo-base"),
        "not a listed member"
    );
}

#[test]
fn a_glob_segment_matches_a_run_and_only_within_its_segment() {
    assert!(wild("demo-*", "demo-core"));
    assert!(wild("*-core", "demo-core"));
    assert!(!wild("demo-*", "other-core"));
    assert!(listed(&["crates/*".to_owned()], "crates/core"));
    assert!(!listed(&["crates/*".to_owned()], "crates/core/deep"));
}

#[test]
fn items_are_the_public_functions_structs_enums_and_traits_that_start_a_line() {
    let text = "pub fn one() {}\npub(crate) async fn two() {}\npub struct Three;\n\
                pub enum Four {}\npub trait Five {}\nfn private() {}\n    pub fn nested() {}\n\
                pub const SIX: u8 = 6;\npub(super) fn seven() {}\n";
    assert_eq!(items(text), ["one", "two", "Three", "Four", "Five"]);
}
