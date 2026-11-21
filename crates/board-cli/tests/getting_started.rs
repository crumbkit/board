//! The shell blocks of docs/guide/getting-started.md run as written, in
//! order, against the `board` this commit builds, in a fresh home. The tests
//! run every `sh` block under "Make your board", "Write a first record" and
//! "Plan a project", and refuse any other fence tag under those sections.
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
fn the_pages_blocks_make_a_board_with_six_plugins_a_project_a_task_and_a_question() {
    let page = page();
    let home = home();
    for section in ["Make your board", "Write a first record", "Plan a project"] {
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
