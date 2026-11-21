//! The shell blocks of docs/guide/getting-started.md run as written, in
//! order, against the `board` this commit builds, in a fresh home. The tests
//! run every `sh` block under "Make your board", "Write a first record",
//! "Plan a project" and "Test something", and refuse any other fence tag
//! under those sections.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the tests run the page's shell blocks as child processes and write the home's git config"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

/// The page, read from the repository root.
fn page() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join("docs/guide/getting-started.md")).unwrap()
}

/// Every fenced block under the `## <section>` heading, in order. Each must
/// be tagged `sh`, so that no block in a tested section goes untested.
fn blocks(page: &str, section: &str) -> Vec<String> {
    let heading = format!("## {section}");
    let mut lines = page.lines().skip_while(|line| *line != heading).skip(1);
    let mut found = Vec::new();
    while let Some(line) = lines.next() {
        if line.starts_with("## ") {
            break;
        }
        if let Some(tag) = line.strip_prefix("```") {
            assert_eq!(tag, "sh", "a block under {heading} is not tagged sh");
            let body: Vec<&str> = lines.by_ref().take_while(|line| *line != "```").collect();
            found.push(body.join("\n"));
        }
    }
    assert!(!found.is_empty(), "no sh block under {heading}");
    found
}

/// A fresh home whose git config names a person, as a colleague's does.
fn home() -> TempDir {
    let home = tempfile::tempdir().unwrap();
    let identity = "[user]\n\tname = Ana Example\n\temail = ana@example.com\n";
    std::fs::write(home.path().join(".gitconfig"), identity).unwrap();
    home
}

/// The directory that holds the built `board`.
fn bin() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_board"))
        .parent()
        .unwrap()
        .to_owned()
}

/// A command that runs in `home` with only that home's settings.
fn command(home: &Path, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", format!("{}:/usr/bin:/bin", bin().display()));
    command
}

/// Runs one block with `sh -e` and asserts it exits 0.
fn run(home: &Path, block: &str) -> Output {
    let output = command(home, "sh")
        .args(["-e", "-c", block])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{block}\nexited {}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// `board --personal --json <arguments>`, which must exit 0.
fn board(home: &Path, arguments: &[&str]) -> Value {
    let output = command(home, env!("CARGO_BIN_EXE_board"))
        .args(["--personal", "--json"])
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn the_pages_blocks_make_a_board_with_six_plugins_a_project_a_task_a_question_and_an_eval() {
    let page = page();
    let home = home();
    let sections = [
        "Make your board",
        "Write a first record",
        "Plan a project",
        "Test something",
    ];
    for section in sections {
        for block in blocks(&page, section) {
            run(home.path(), &block);
        }
    }

    let plugins = board(home.path(), &["plugin", "list", "--fields", "name,enabled"]);
    let mut enabled: Vec<&str> = plugins["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["enabled"] == true)
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    enabled.sort_unstable();
    assert_eq!(
        enabled,
        ["evals", "plans", "rules", "runs", "share", "tasks"]
    );
    board(home.path(), &["check"]);

    let tree = board(
        home.path(),
        &["tasks", "tree", "--path", "/tasks/my-first-project.md"],
    );
    let project = &tree["data"]["output"]["tree"];
    assert_eq!(project["kind"], "project", "{tree}");
    assert_eq!(project["title"], "My first project");
    let children = project["children"].as_array().unwrap();
    assert_eq!(children.len(), 1, "{tree}");
    assert_eq!(children[0]["title"], "My first task");

    let open = board(
        home.path(),
        &["plans", "open", "--path", "/projects/my-first-project"],
    );
    let questions = open["data"]["output"]["questions"].as_array().unwrap();
    assert!(
        questions.is_empty(),
        "the answered question is still open: {open}"
    );
    let answers = board(home.path(), &["ls", "/projects/my-first-project/answers"]);
    assert!(
        answers
            .to_string()
            .contains("/projects/my-first-project/answers/"),
        "{answers}"
    );
    let asked = board(home.path(), &["ls", "/projects/my-first-project/questions"]);
    let listed = asked.to_string();
    let start = listed
        .find("/projects/my-first-project/questions/")
        .unwrap();
    let path = listed[start..].split('"').next().unwrap();
    let got = board(home.path(), &["get", path]);
    let question: Value = serde_json::from_str(got["data"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(question["by"]["person"], "ana@example.com", "{question}");
    let decisions = board(home.path(), &["ls", "/projects/my-first-project/decisions"]);
    assert!(
        decisions
            .to_string()
            .contains("/projects/my-first-project/decisions/"),
        "{decisions}"
    );

    let about = board(home.path(), &["evals", "about", "--subject", "/topics/wc"]);
    let evals = about["data"]["output"]["evals"].as_array().unwrap();
    assert_eq!(evals.len(), 1, "{about}");
    assert_eq!(evals[0]["path"], "/evals/wc-last-line", "{about}");
    assert_eq!(evals[0]["status"], "concluded", "{about}");
    assert_eq!(evals[0]["verdict"], "refuted", "{about}");
    let latest = board(
        home.path(),
        &["runs", "latest", "--suite", "/suites/wc-last-line"],
    );
    let run = &latest["data"]["output"]["run"];
    let path = run["path"].as_str().unwrap_or_default();
    assert!(path.starts_with("/suites/wc-last-line/runs/"), "{latest}");
    assert_eq!(run["outcome"], "pass", "{latest}");
    let got = board(home.path(), &["get", "/evals/wc-last-line"]);
    let eval: Value = serde_json::from_str(got["data"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(eval["runs"], serde_json::json!([path]), "{eval}");
}

/// Whether any entry `board ls` printed, at any depth of its JSON, is the
/// directory whose logical path is `/repos`.
fn lists_repos(listed: &Value) -> bool {
    match listed {
        Value::Object(fields) => {
            fields.get("logical") == Some(&Value::from("/repos"))
                || fields.values().any(lists_repos)
        }
        Value::Array(items) => items.iter().any(lists_repos),
        _ => false,
    }
}

/// A home with the board the page's "Make your board" blocks make.
fn made_board() -> TempDir {
    let page = page();
    let home = home();
    for block in blocks(&page, "Make your board") {
        run(home.path(), &block);
    }
    home
}

/// The evals skill tells an agent that a board whose top level lists no
/// `repos` directory mounts no repositories, so `board ls /` must answer in
/// a fresh board and list none.
#[test]
fn board_ls_of_the_root_answers_in_a_fresh_board_and_lists_no_repos_directory() {
    let home = made_board();
    let listed = board(home.path(), &["ls", "/"]);
    assert!(!lists_repos(&listed), "{listed}");
}

#[test]
fn board_ls_of_the_root_lists_the_repos_directory_once_the_board_mounts_a_repository() {
    let home = made_board();
    let repo = home.path().join("x");
    run(
        home.path(),
        "git init -q -b main x && git -C x commit -q --allow-empty -m first",
    );
    let commit = run(home.path(), "git -C x rev-parse HEAD");
    let commit = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
    let mount = serde_json::json!({
        "$type": "board/mount/v1",
        "repo": repo.to_str().unwrap(),
        "follow": "main",
        "commit": commit,
    });
    run(
        home.path(),
        &format!("echo '{mount}' | board --personal put /repos/x"),
    );
    let listed = board(home.path(), &["ls", "/"]);
    assert!(lists_repos(&listed), "{listed}");
}

/// The text under `heading` in the page, up to the next heading of any
/// level.
fn section<'page>(page: &'page str, heading: &str) -> &'page str {
    let start = page.find(&format!("\n{heading}\n")).unwrap() + heading.len() + 2;
    let rest = &page[start..];
    let end = rest.find("\n#").map_or(rest.len(), |end| end + 1);
    &rest[..end]
}

#[test]
fn the_release_folder_subsection_names_its_installer_and_the_hooks_board_app_runs() {
    let page = page();
    let release = section(&page, "### Install from a release folder");
    let release = release.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(release.contains("`<release folder>/install`"), "{release}");
    assert!(
        release.contains("`<release folder>/install --hooks`"),
        "{release}"
    );
    assert!(
        release.contains("Board.app always runs `install --hooks`"),
        "{release}"
    );
    assert!(
        page.lines()
            .take(4)
            .any(|line| line.contains("release folder"))
    );
}

#[test]
fn the_install_section_still_takes_a_reader_through_the_dmg_and_board_app() {
    let page = page();
    let dmg = section(&page, "## Install");
    assert!(dmg.contains("Open the dmg"), "{dmg}");
    assert!(dmg.contains("run Board.app"), "{dmg}");
    assert!(!dmg.contains("release folder"), "{dmg}");
}

/// A failed `put --new` prints no path, so the block must stop before it
/// concludes the eval with an empty run.
#[test]
fn the_test_something_block_stops_when_the_run_has_no_path() {
    let page = page();
    let blocks = blocks(&page, "Test something");
    let lines: Vec<&str> = blocks.iter().flat_map(|block| block.lines()).collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with("run=$("))
        .unwrap();
    assert_eq!(lines[at + 1], "[ -n \"$run\" ]");
}

#[test]
fn the_config_block_leaves_an_existing_config_file_unchanged_and_says_it_exists() {
    let page = page();
    let home = home();
    let config = home.path().join(".config/board/config.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let other = "{\"boards\": [\"/elsewhere\"]}\n";
    std::fs::write(&config, other).unwrap();

    let make = blocks(&page, "Make your board");
    let writes_config = |block: &&String| block.contains("config.json");
    let config_block = make.iter().find(writes_config).unwrap();
    for block in make.iter().take_while(|block| !writes_config(block)) {
        run(home.path(), block);
    }
    let output = run(home.path(), config_block);

    assert_eq!(std::fs::read_to_string(&config).unwrap(), other);
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(said.contains("exists"), "{said}");
}

#[test]
fn the_readme_links_the_page_and_the_docs_map_names_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    assert!(read("README.md").contains("](docs/guide/getting-started.md)"));
    assert!(read("DOCS.md").contains("`docs/guide/getting-started.md`"));
}
