//! The desk the sending actions' tests run at: a personal board with
//! controls, boundary and github enabled, named in a known boards file, a
//! checkout it mounts at /repos/widgets whose origin names GitHub, and a
//! stub gh first on `PATH`.
//!
//! The checkout's git config sends every fetch and push to a local bare
//! repository in place of GitHub, so no test reaches a network. `HOME` is a
//! directory of the desk's own, so no global git config applies.
#![expect(
    clippy::disallowed_methods,
    reason = "the desk writes its fixture files outside any board, as the pytest fixture did"
)]

use std::path::PathBuf;
use std::time::Duration;

use board_plugin::child::{Process, run as start};
use board_scenario::signer::Signer;

use super::*;

/// The stub gh of [`GH`], which also keeps its stdin in `gh-stdin.txt` and
/// its environment in `gh-env.txt`.
fn gh_sending() -> String {
    GH.replace(
        "echo call >> \"$here/gh-calls.txt\"",
        "echo call >> \"$here/gh-calls.txt\"\ncat > \"$here/gh-stdin.txt\"\nenv > \"$here/gh-env.txt\"",
    )
}

/// The remote a checkout names. Its own config sends git to a local bare
/// repository in its place.
pub(super) const ORIGIN: &str = "https://github.com/Octo/Widgets.git";
/// A pattern a test's control refuses, and the control's path in the
/// personal board.
const PRIVATE: &str = r"acme-\d{3}";
pub(super) const NAMES: &str = "/controls/names";
/// The destination every check of the checkout names.
pub(super) const TO: &str = "github:octo/widgets";

pub(super) struct Desk {
    /// Holds the home, the checkout, the remote, the stub gh and the cache.
    root: tempfile::TempDir,
    pub(super) personal: Scenario,
    signer: Signer,
}

impl Desk {
    /// A checkout on `main`, pushed to its remote, mounted at
    /// /repos/widgets in a personal board.
    pub(super) fn new() -> Self {
        let desk = Self {
            root: tempfile::tempdir().unwrap(),
            personal: Scenario::new(),
            signer: Signer::new(),
        };
        for directory in ["home/config/board", "bin", "cache", "widgets"] {
            std::fs::create_dir_all(desk.root.path().join(directory)).unwrap();
        }
        let gh = desk.bin().join("gh");
        put_file(&gh, &gh_sending());
        executable(&gh);
        let personal = desk.personal.path();
        desk.known(&json!({"boards": [personal], "personal": personal}));
        let remote = desk.remote();
        let remote = remote.to_str().unwrap();
        desk.git(
            desk.root.path(),
            &["init", "-q", "--bare", "-b", "main", remote],
        );
        let checkout = desk.checkout();
        desk.git(&checkout, &["init", "-q", "-b", "main"]);
        desk.git(&checkout, &["remote", "add", "origin", ORIGIN]);
        let rewrite = format!("url.{remote}.insteadOf");
        desk.git(&checkout, &["config", &rewrite, ORIGIN]);
        let head = desk.commit("Start");
        desk.git(&checkout, &["push", "-q", "-u", "origin", "main"]);
        let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
        desk.personal.write(".board/board.json", Item::File(config));
        let mount = json!({
            "$type": "board/mount/v1",
            "repo": checkout,
            "follow": "origin/main",
            "commit": head,
        });
        desk.write("repos/widgets.json", &mount.to_string());
        for plugin in ["controls", "boundary", "github"] {
            let enabled = desk.run_in(desk.personal.workdir(), &["plugin", "enable", plugin], &[]);
            assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
        }
        desk
    }

    pub(super) fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    pub(super) fn checkout(&self) -> PathBuf {
        // Canonical, as git names a checkout's root, so /tmp and /private/tmp agree.
        self.root.path().canonicalize().unwrap().join("widgets")
    }

    pub(super) fn remote(&self) -> PathBuf {
        self.root.path().join("remote.git")
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// Writes the known boards file.
    pub(super) fn known(&self, known: &Value) {
        let file = self.home().join("config/board/config.json");
        std::fs::write(file, known.to_string()).unwrap();
    }

    /// The `PATH` the desk's processes see: the stub gh, the `board` cargo
    /// built, then the system's.
    fn path(&self) -> String {
        let board = PathBuf::from(crate::tests::help::binary());
        let board = board.parent().unwrap().display().to_string();
        format!("{}:{board}:/usr/bin:/bin", self.bin().display())
    }

    /// The variables a shell would give `board` or git at the desk, with
    /// `extra` over them.
    pub(super) fn variables(&self, extra: &[(&str, &str)]) -> Vec<(String, OsString)> {
        let home = self.home();
        let mut variables: Vec<(String, OsString)> = [
            ("PATH", OsString::from(self.path())),
            ("HOME", home.clone().into_os_string()),
            ("XDG_CONFIG_HOME", home.join("config").into_os_string()),
            (
                "XDG_CACHE_HOME",
                self.root.path().join("cache").into_os_string(),
            ),
            ("XDG_DATA_HOME", home.join("data").into_os_string()),
            ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        for (name, value) in extra {
            variables.retain(|(each, _)| each != name);
            variables.push(((*name).to_owned(), OsString::from(value)));
        }
        variables
    }

    /// Runs `program` in `at` with the desk's variables and `extra`.
    pub(super) fn process(
        &self,
        program: &str,
        at: &Path,
        arguments: &[&str],
        extra: &[(&str, &str)],
    ) -> (i32, String, String) {
        let arguments: Vec<String> = arguments.iter().map(|each| (*each).to_owned()).collect();
        let process = Process {
            program,
            arguments: &arguments,
            directory: at,
            input: b"",
            deadline: Duration::from_mins(2),
        };
        let exited = start(&process, &self.variables(extra)).unwrap();
        (
            exited.code.unwrap_or(-1),
            text(&exited.stdout),
            text(&exited.stderr),
        )
    }

    /// What git prints in `at`, trimmed; it must succeed. Commits are made
    /// as `t <t@t>`.
    pub(super) fn git(&self, at: &Path, arguments: &[&str]) -> String {
        let identity = ["-c", "user.name=t", "-c", "user.email=t@t"];
        let arguments: Vec<&str> = identity
            .into_iter()
            .chain(arguments.iter().copied())
            .collect();
        let (code, stdout, stderr) = self.process("git", at, &arguments, &[]);
        assert_eq!(code, 0, "git {arguments:?}: {stderr}");
        stdout.trim().to_owned()
    }

    /// A raw `git push` in the checkout, as a shell would run it, so its
    /// hook finds board: its exit code and stderr.
    pub(super) fn push(&self, arguments: &[&str], extra: &[(&str, &str)]) -> (i32, String) {
        self.push_in(&self.checkout(), arguments, extra)
    }

    pub(super) fn push_in(
        &self,
        at: &Path,
        arguments: &[&str],
        extra: &[(&str, &str)],
    ) -> (i32, String) {
        let arguments: Vec<&str> = ["push", "-q"]
            .into_iter()
            .chain(arguments.iter().copied())
            .collect();
        let (code, _, stderr) = self.process("git", at, &arguments, extra);
        (code, stderr)
    }

    /// Commits `message` in the checkout, changing `file` to `text`, or to
    /// the message when none is given, and returns the commit's full SHA.
    pub(super) fn commit_to(&self, message: &str, file: &str, text: Option<&str>) -> String {
        let checkout = self.checkout();
        let body = text.map_or_else(|| format!("{message}\n"), str::to_owned);
        put_file(&checkout.join(file), &body);
        self.git(&checkout, &["add", file]);
        self.git(&checkout, &["commit", "-q", "-m", message]);
        self.git(&checkout, &["rev-parse", "HEAD"])
    }

    pub(super) fn commit(&self, message: &str) -> String {
        self.commit_to(message, "widget.txt", None)
    }

    /// The commit the checkout's branch is at.
    pub(super) fn head(&self) -> String {
        self.git(&self.checkout(), &["rev-parse", "HEAD"])
    }

    /// The commit the bare remote's `branch` points at, or `None`.
    pub(super) fn remote_head(&self, branch: &str) -> Option<String> {
        let reference = format!("refs/heads/{branch}");
        let arguments = ["rev-parse", "--verify", "-q", &reference];
        let (_, stdout, _) = self.process("git", &self.remote(), &arguments, &[]);
        Some(stdout.trim().to_owned()).filter(|sha| !sha.is_empty())
    }

    /// Runs `board` in `directory`, which Board names in `BOARD_CALLER_DIR`,
    /// with the desk's known boards file, signer, cache and variables, and
    /// `extra` among the variables a capability may name.
    pub(super) fn run_in(
        &self,
        directory: &Path,
        arguments: &[&str],
        extra: &[(&str, &str)],
    ) -> Output {
        let home = self.home();
        let inherited: board_plugin::Inherited = (self.variables(extra).into_iter())
            .map(|(name, value)| (OsString::from(name), value))
            .collect();
        let context = Context {
            start: directory.to_path_buf(),
            home: None,
            cache: Some(self.root.path().join("cache")),
            plugins: None,
            boards: Some(home.join("config/board/config.json")),
            signer: Some(self.signer.program().into_os_string()),
            allowed_signers: Some(self.signer.allowed()),
            environment: board_plugin::Environment {
                path: Some(self.path().into()),
                home: Some(home.clone().into_os_string()),
                config: Some(home.join("config").into_os_string()),
                cache: Some(self.root.path().join("cache").into_os_string()),
                data: Some(home.join("data").into_os_string()),
                caller: Some(directory.as_os_str().to_owned()),
                inherited,
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

    /// `board [--personal] --json call github.<capability> <flags>` in
    /// `directory`, the checkout when none is given: its run and its one
    /// document.
    pub(super) fn call_at(
        &self,
        directory: Option<&Path>,
        personal: bool,
        capability: &str,
        flags: &[&str],
        extra: &[(&str, &str)],
    ) -> (Output, Value) {
        let target = format!("github.{capability}");
        let front: &[&str] = if personal { &["--personal"] } else { &[] };
        let arguments: Vec<&str> = (front.iter().copied())
            .chain(["--json", "call", &target])
            .chain(flags.iter().copied())
            .collect();
        let checkout = self.checkout();
        let ran = self.run_in(directory.unwrap_or(&checkout), &arguments, extra);
        let document = json(&ran);
        (ran, document)
    }

    /// `board --json call github.<capability> <flags>` in the checkout.
    pub(super) fn call(&self, capability: &str, flags: &[&str]) -> (Output, Value) {
        self.call_at(None, false, capability, flags, &[])
    }

    /// The output of a call that must succeed.
    pub(super) fn output(&self, capability: &str, flags: &[&str]) -> Value {
        let (ran, document) = self.call(capability, flags);
        assert_eq!(ran.code, 0, "{document}");
        document["data"]["output"].clone()
    }

    /// Writes a file at `path` in the personal board.
    pub(super) fn write(&self, path: &str, text: &str) {
        self.personal
            .write(path.trim_start_matches('/'), Item::File(text));
    }

    /// The record at the logical `path` in the personal board, or `None`.
    pub(super) fn record(&self, path: &str) -> Option<Value> {
        let file = (self.personal.workdir()).join(format!("{}.json", path.trim_start_matches('/')));
        let text = std::fs::read_to_string(file).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }

    /// A control that refuses [`PRIVATE`], and a boundary on octo's
    /// repositories that checks `parts` of `actions` against it.
    pub(super) fn refuse(&self, parts: &[&str], actions: &str) {
        self.write("private/names.txt", &format!("{PRIVATE}\n"));
        let control = json!({
            "$type": "controls/control/v1",
            "requirement": "A test.",
            "purpose": "A test.",
            "rule": {"pattern": "/private/names.txt", "show": "match-only"},
        });
        self.write(&format!("{NAMES}.json"), &control.to_string());
        let boundary = json!({
            "$type": "boundary/boundary/v1",
            "to": "github:octo/*",
            "actions": [actions],
            "checks": [{"control": NAMES, "parts": parts}],
        });
        self.write("boundaries/octo.json", &boundary.to_string());
    }

    /// A judged control on `parts` of every github action, and a stub agent
    /// in the desk's home that reads its prompt, runs `script` and then
    /// answers with no findings. Returns the agent's argv as
    /// `BOARD_CONTROLS_AGENT` takes it, naming a model so verdicts are kept.
    pub(super) fn judged(&self, parts: &[&str], script: &str) -> String {
        self.write("rubrics/plain.md", "Write plainly.\n");
        let control = json!({
            "$type": "controls/control/v1",
            "requirement": "A test.",
            "purpose": "A test.",
            "rule": {"judged": "/rubrics/plain.md"},
        });
        self.write("controls/plain.json", &control.to_string());
        let boundary = json!({
            "$type": "boundary/boundary/v1",
            "to": "github:*",
            "actions": ["github.*"],
            "checks": [{"control": "/controls/plain", "parts": parts}],
        });
        self.write("boundaries/octo.json", &boundary.to_string());
        let agent = self.home().join("agent");
        let findings = r#"{"findings": []}"#;
        put_file(
            &agent,
            &format!("#!/bin/sh\ncat > /dev/null\n{script}\necho '{findings}'\n"),
        );
        executable(&agent);
        json!([agent.display().to_string(), "--model", "stub-model-1"]).to_string()
    }

    /// Makes the stub gh print `answer`, as JSON unless it is text, and
    /// `stderr`, and exit with `code`.
    pub(super) fn gh_answers(&self, answer: &Value, code: u8, stderr: &str) {
        let out = answer
            .as_str()
            .map_or_else(|| answer.to_string(), str::to_owned);
        std::fs::write(self.bin().join("gh-out.txt"), out).unwrap();
        std::fs::write(self.bin().join("gh-err.txt"), stderr).unwrap();
        std::fs::write(self.bin().join("gh-exit.txt"), code.to_string()).unwrap();
    }

    /// Makes the stub gh also run `script` before it answers.
    pub(super) fn gh_also(&self, script: &str) {
        let gh = gh_sending().replace(
            "cat \"$here/gh-out.txt\"",
            &format!("{script}\ncat \"$here/gh-out.txt\""),
        );
        put_file(&self.bin().join("gh"), &gh);
    }

    /// How many times the stub gh ran.
    pub(super) fn gh_calls(&self) -> usize {
        std::fs::read_to_string(self.bin().join("gh-calls.txt"))
            .map_or(0, |calls| calls.lines().count())
    }

    /// A file the stub gh wrote in its last run.
    pub(super) fn gh_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.bin().join(name)).unwrap()
    }

    /// The arguments of the stub gh's last run.
    pub(super) fn gh_argv(&self) -> Vec<String> {
        self.gh_file("gh-argv.txt")
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// The digest of content sent to [`TO`], as the boundary's README defines
/// it.
pub(super) fn digest(action: &str, parts: &Value) -> String {
    let content = json!({"action": action, "parts": parts, "to": TO});
    board_model::Digest::of(content.to_string().as_bytes()).hex()
}

/// Writes `text` to the file at `path`, anywhere at the desk.
pub(super) fn put_file(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Makes the file at `path` executable.
pub(super) fn executable(path: &Path) {
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

/// Removes the file at `path`.
pub(super) fn remove(path: &Path) {
    std::fs::remove_file(path).unwrap();
}
