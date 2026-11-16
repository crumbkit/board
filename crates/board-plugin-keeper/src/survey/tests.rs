//! A synthetic workspace: members by glob, an excluded directory, a renamed
//! dependency and a dependency that is not a member.

use serde_json::Value;

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

/// A host whose every query passes the output budget.
struct OverBudget(board_ops::Settings);

impl Host for OverBudget {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        Ok(board_ops::QueryReport {
            columns: vec!["logical".into()],
            rows: Vec::new(),
            truncated: true,
            over_budget: true,
            memory: None,
        })
    }

    fn refs(
        &self,
        _: &[board_model::LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        unreachable!("the survey resolves no paths")
    }

    fn get_each(
        &self,
        _: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        unreachable!("the survey reads no files after its listing fails")
    }

    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        unreachable!("the survey writes nothing")
    }

    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        unreachable!("the survey calls no plugin")
    }

    fn board(
        &self,
        _: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        unreachable!("the survey opens no other board")
    }

    fn decide(
        &self,
        _: &str,
        _: &board_model::LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        unreachable!("the survey asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("the survey syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("the survey updates no mount")
    }

    fn settings(&self) -> &board_ops::Settings {
        &self.0
    }
}

#[test]
fn a_mount_with_more_files_than_the_budget_allows_is_left_without_crates_and_says_why() {
    let host = OverBudget(board_ops::Settings {
        now: std::time::SystemTime::UNIX_EPOCH,
        day_start: None,
        utc_offset: 0,
        caller: None,
        environment: board_ops::Variables::default(),
        cache_home: None,
    });
    let surveyed = survey(&host, "/repos/big").expect("a large mount is no refusal");
    let problem = surveyed.expect_err("a large mount has no crates");
    assert!(problem.contains("too many files"), "{problem}");
}
