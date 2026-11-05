//! The review plugin through the binary, against real git repositories: a
//! repository that is its own board and enables review, the owner's
//! personal board, which takes the plugin's asks, a stand-in agent and a
//! stand-in gh. These were the plugin's pytest suite.
//!
//! The stand-in agent writes the prompt it reads to `prompt.txt`, its
//! environment to `env.txt` and its directory and what is in it to
//! `directory.txt`, prints `answer.txt` and exits with the code in
//! `exit.txt`, so no test runs a model. The stand-in gh is first on the
//! plugin's `PATH`. It records its arguments in `gh-argv.txt`, its stdin in
//! `gh-stdin.txt` and each run in `gh-calls.txt`, and prints a comment's
//! URL, so no test reaches GitHub.

use super::*;

mod covered;
mod post;
mod run;
mod subject;

const AGENT: &str = r#"#!/bin/sh
here="$(dirname "$0")"
cat > "$here/prompt.txt"
env > "$here/env.txt"
{ pwd; ls -A; } > "$here/directory.txt"
cat "$here/answer.txt"
exit "$(cat "$here/exit.txt" 2>/dev/null || echo 0)"
"#;

/// The comment URL the stand-in gh prints, unless `gh-url.txt` holds
/// another.
const COMMENT_URL: &str = "https://github.com/o/r/pull/7#issuecomment-1";

const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
printf '%s\n' "$@" > "$here/gh-argv.txt"
cat > "$here/gh-stdin.txt"
echo call >> "$here/gh-calls.txt"
code="$(cat "$here/gh-exit.txt" 2>/dev/null || echo 0)"
if [ "$code" != 0 ]; then echo "HTTP 502: Bad Gateway" >&2; else cat "$here/gh-url.txt" 2>/dev/null || echo "https://github.com/o/r/pull/7#issuecomment-1"; fi
exit "$code"
"#;

/// Runs git in `directory` without the user's or the system's config and
/// returns what it printed, trimmed.
#[expect(
    clippy::disallowed_methods,
    reason = "the fixture's commits, branches, merges and rebases are made with git, as a person makes them"
)]
fn git(directory: &Path, arguments: &[&str]) -> String {
    let ran = std::process::Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "git {arguments:?} in {}: {}{}",
        directory.display(),
        text(&ran.stdout),
        text(&ran.stderr)
    );
    text(&ran.stdout).trim().to_owned()
}

/// A stand-in git for the plugin's `PATH`, whose first entry is its own
/// directory: it runs `before`, then the git that follows on `PATH`.
fn stand_in_git(before: &str) -> String {
    format!(
        "#!/bin/sh\n{before}\nhere=$(dirname \"$0\")\nPATH=${{PATH#\"$here:\"}}\nexport PATH\nexec git \"$@\"\n"
    )
}

/// Commits everything in `directory` and returns the commit.
fn commit_all(directory: &Path, message: &str) -> String {
    git(directory, &["add", "-A"]);
    git(directory, &["commit", "-qm", message]);
    git(directory, &["rev-parse", "HEAD"])
}

/// A new repository holding `files`, committed once on main, and a mount
/// record pinned to that commit.
fn library(files: &[(&str, &str)]) -> (Scenario, Value) {
    let library = Scenario::new();
    git(
        library.workdir(),
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    identify(library.workdir());
    for (path, text) in files {
        library.write(path, Item::File(text));
    }
    let commit = commit_all(library.workdir(), "library");
    let mount = json!({"$type": "board/mount/v1", "repo": library.path(), "follow": "main", "commit": commit});
    (library, mount)
}

fn identify(directory: &Path) {
    git(directory, &["config", "user.name", "t"]);
    git(directory, &["config", "user.email", "t@t"]);
}

/// An agent's answer: some prose, then the JSON object `review.run` reads.
fn answer(verdict: &str, findings: &Value) -> String {
    let object = json!({"verdict": verdict, "findings": findings});
    format!("Looks fine.\n\n```json\n{object}\n```\n")
}

fn approving() -> String {
    answer("approve", &json!([]))
}

/// A repository on main with one commit, whose own board enables review,
/// the owner's personal board, the stand-in agent and the stand-in gh.
struct Repo {
    board: Scenario,
    agent: Scenario,
    bin: Scenario,
    personal: Personal,
}

impl Repo {
    fn new() -> Self {
        let repo = Self {
            board: Scenario::new(),
            agent: Scenario::new(),
            bin: Scenario::new(),
            personal: Personal::enabling(&[]),
        };
        let root = repo.root();
        git(root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        identify(root);
        repo.agent.write("agent.sh", Item::Executable(AGENT));
        repo.answers(&approving(), 0);
        repo.bin.write("gh", Item::Executable(GH));
        repo.write(".board/board.json", r#"{"$type": "board/config/v1"}"#);
        repo.commit("README.md", "A repository.\n");
        let enabled = repo.run(&["plugin", "enable", "review"], &[]);
        assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        // Enabling commits the types itself.
        assert_eq!(git(root, &["status", "--porcelain"]), "");
        repo
    }

    fn root(&self) -> &Path {
        self.board.workdir()
    }

    /// Writes a fixture file at `path` under the board's root.
    fn write(&self, path: &str, text: &str) {
        self.board.write(path, Item::File(text));
    }

    fn git(&self, arguments: &[&str]) -> String {
        git(self.root(), arguments)
    }

    /// Writes `text` at `path`, commits it, and returns the commit.
    fn commit(&self, path: &str, text: &str) -> String {
        self.write(path, text);
        // A rewrite of the same size within the index's second can look
        // unchanged to git, so the file is hashed again.
        git(
            self.root(),
            &["rm", "-q", "--cached", "--ignore-unmatch", path],
        );
        commit_all(self.root(), &format!("change {path}"))
    }

    /// Makes the stand-in agent print `stdout` and exit with `code`.
    fn answers(&self, stdout: &str, code: u8) {
        self.agent.write("answer.txt", Item::File(stdout));
        self.agent.write("exit.txt", Item::File(&code.to_string()));
    }

    /// What the stand-in agent last read.
    fn prompt(&self) -> String {
        self.agent_file("prompt.txt")
    }

    fn agent_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.agent.workdir().join(name)).unwrap()
    }

    fn agent_path(&self) -> String {
        self.agent.workdir().join("agent.sh").display().to_string()
    }

    /// Runs `board` in the board's root with the personal board, the
    /// signer, the stand-in gh first on `PATH`, `BOARD_REVIEW_AGENT` naming
    /// the stand-in agent, and `extra` among the variables Board inherited.
    /// An empty value in `extra` leaves its variable unset.
    fn run(&self, arguments: &[&str], extra: &[(&str, &str)]) -> Output {
        let agent = json!(["sh", self.agent_path()]).to_string();
        let mut inherited = vec![("BOARD_REVIEW_AGENT", agent.as_str())];
        for (name, value) in extra {
            inherited.retain(|(held, _)| held != name);
            if !value.is_empty() {
                inherited.push((name, value));
            }
        }
        let personal = &self.personal;
        let context = Context {
            start: self.root().to_path_buf(),
            home: None,
            cache: Some(personal.cache.path().to_path_buf()),
            plugins: None,
            boards: Some(personal.xdg.workdir().join("board/config.json")),
            signer: Some(personal.signer.program().into_os_string()),
            allowed_signers: Some(personal.signer.allowed()),
            environment: board_plugin::Environment {
                path: Some(format!("{}:{PATH}", self.bin.workdir().display()).into()),
                inherited: (inherited.iter())
                    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                    .collect(),
                ..board_plugin::Environment::default()
            },
            complete: None,
            terminal: false,
            browser: None,
            binary: None,
            cursor: None,
            built_in: board_plugins::BUILT_IN,
        };
        let arguments = std::iter::once(BINARY_NAME)
            .chain(arguments.iter().copied())
            .map(OsString::from);
        run(arguments, &context, &mut &b""[..], &mut Ran::default())
    }

    /// `board --json call review.<capability> <flags>`: its exit code, and
    /// its output or its failure.
    fn call(&self, capability: &str, flags: &[&str]) -> (u8, Value) {
        self.call_with(capability, flags, &[])
    }

    fn call_with(&self, capability: &str, flags: &[&str], extra: &[(&str, &str)]) -> (u8, Value) {
        let target = format!("review.{capability}");
        let arguments: Vec<&str> = ["--json", "call", &target]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let ran = self.run(&arguments, extra);
        let document = json(&ran);
        if ran.code == 0 {
            (0, document["data"]["output"].clone())
        } else {
            (ran.code, document)
        }
    }

    /// The record at the logical `path`, from the working tree.
    fn get(&self, path: &str) -> Value {
        let file = self
            .root()
            .join(format!("{}.json", path.trim_start_matches('/')));
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    /// Commits a brief at /reviews/<name>/brief for `range` with `fields`,
    /// and returns its logical path.
    fn brief(&self, name: &str, range: &str, fields: &Value) -> String {
        let base =
            json!({"$type": "review/brief/v1", "range": range, "focus": "Check the sync change."});
        self.commit(&format!("reviews/{name}/brief.json"), &record(base, fields));
        format!("/reviews/{name}/brief")
    }

    /// Each review record's code from `board check --healthy --all`, by its path;
    /// `None` when it is whole.
    fn checked(&self) -> BTreeMap<String, Option<String>> {
        let report = json(&self.run(&["--json", "check", "--healthy", "--all"], &[]));
        (report["data"].as_array().unwrap().iter())
            .filter_map(|entry| {
                let logical = entry["logical"].as_str()?;
                let code = entry["code"].as_str().map(str::to_owned);
                logical
                    .starts_with("/reviews/")
                    .then(|| (logical.to_owned(), code))
            })
            .collect()
    }
}

#[test]
fn board_lists_the_plugin_as_working_with_its_types_installed() {
    let repo = Repo::new();
    let listed = json(&repo.run(&["--json", "plugin", "list"], &[]));
    let review = (listed["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "review")
        .unwrap()
        .clone();
    assert_eq!(review["status"], "ok", "{review}");
    let checked = json(&repo.run(&["--json", "check", "--healthy", "--all"], &[]));
    let status = (checked["data"].as_array().unwrap().iter())
        .find(|entry| entry["logical"] == "/.board/types/review/report/v1.schema")
        .map(|entry| entry["status"].clone());
    assert_eq!(status, Some(json!("ok")));
}

#[test]
fn every_capability_states_its_effect_hints_as_its_code_has_them() {
    let repo = Repo::new();
    let listed = json(&repo.run(&["--json", "plugin", "list"], &[]));
    let review = (listed["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "review")
        .unwrap()
        .clone();
    let hints: Vec<(String, Value)> = (review["capabilities"].as_array().unwrap().iter())
        .map(|capability| {
            (
                capability["name"].as_str().unwrap().to_owned(),
                capability["annotations"].clone(),
            )
        })
        .collect();
    let stated = |read_only: bool, destructive: bool, idempotent: bool, open_world: bool| {
        json!({
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": idempotent,
            "openWorldHint": open_world,
        })
    };
    assert_eq!(
        hints,
        [
            ("run".to_owned(), stated(false, false, false, true)),
            ("post".to_owned(), stated(false, false, false, true)),
            ("covered".to_owned(), stated(true, false, true, false)),
        ]
    );
}
