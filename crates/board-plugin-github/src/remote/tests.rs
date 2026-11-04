//! The URL rule the sending actions, the pre-push hook and `covers` share:
//! which URLs reach GitHub, which repository they name, and which cannot be
//! read; and the destinations of a push to a remote.

use std::ffi::OsString;
use std::time::{Duration, SystemTime};

use board_ops::{Run, Settings, Variables};
use board_plugin::child::{Exited, ProcessError};
use board_plugin::child::{Process, run as start};

use super::*;

/// The hosts `ssh -G` resolves in [`named`]: `work-gh` is an alias of
/// github.com, `elsewhere` of another host.
const ALIASES: &[(&str, &str)] = &[("work-gh", "github.com"), ("elsewhere", "git.example.com")];

fn named(url: &str) -> Option<(String, String)> {
    github_repository(url, &Resolver::Ssh(&aliased(ALIASES))).unwrap()
}

/// What a URL of octo/widgets names.
fn octo() -> (String, String) {
    ("octo".to_owned(), "widgets".to_owned())
}

#[test]
fn every_form_of_a_github_url_names_its_repository() {
    for url in [
        "https://github.com/Octo/Widgets.git",
        "https://GitHub.com/octo/widgets",
        "https://github.com:443/octo/widgets.git",
        "https://user:secret@www.github.com/octo/widgets/",
        "git@github.com:octo/widgets.git",
        "ssh://git@ssh.github.com:443/octo/widgets.git",
        "ssh://git@github.com:22/octo/widgets",
        "git@github.com-work:octo/widgets.git",
        "git@work-gh:octo/widgets.git",
        "git://github.com/octo/widgets.git",
        "[github.com]:octo/widgets",
        "git@[github.com]:octo/widgets.git",
        "ssh://git@[github.com]/octo/widgets",
        "[github.com:22]:octo/widgets",
        "https://github.com./octo/widgets",
        "git@github.com.:octo/widgets.git",
    ] {
        assert_eq!(named(url), Some(octo()), "{url}");
    }
}

#[test]
fn a_local_path_or_another_host_is_not_github_and_a_github_alias_is() {
    for url in [
        "/srv/git/widgets.git",
        "../widgets.git",
        "file:///srv/widgets.git",
        "https://gitlab.example/o/w.git",
        "https://github.com-evil.example/octo/widgets",
        "git@elsewhere:octo/widgets.git",
        "octo/widgets",
    ] {
        assert_eq!(named(url), None, "{url}");
    }
    assert_eq!(named("git@work-gh:octo/widgets.git"), Some(octo()));
}

#[test]
fn a_url_that_could_reach_github_but_cannot_be_read_is_refused_and_a_resolved_alias_is_admitted() {
    for url in [
        "git@unknown-alias:octo/widgets.git",
        "https://github.com/octo",
        "ext::ssh github.com",
        "git@-oX:a/b",
    ] {
        let refused = github_repository(url, &Resolver::Ssh(&aliased(ALIASES))).unwrap_err();
        assert_eq!(refused.code, "github::remote::unknown", "{url}");
    }
    assert_eq!(named("git@work-gh:octo/widgets.git"), Some(octo()));
}

#[test]
fn each_github_url_form_names_owner_and_repository_and_its_non_github_twin_names_none() {
    let itself = &[
        ("gitlab.com", "gitlab.com"),
        ("github.example", "github.example"),
        ("example.com", "example.com"),
        ("github.org", "github.org"),
        ("github.com", "github.com"),
    ];
    let host = aliased(itself);
    let resolver = Resolver::Ssh(&host);
    for (github, other) in [
        (
            "https://github.com/Octo/Widgets.git",
            "https://gitlab.com/Octo/Widgets.git",
        ),
        (
            "https://token@github.com/octo/widgets",
            "https://token@github.example/octo/widgets",
        ),
        (
            "git@github.com:Octo/Widgets.git",
            "git@gitlab.com:Octo/Widgets.git",
        ),
        (
            "ssh://git@github.com/octo/widgets.git",
            "ssh://git@example.com/octo/widgets.git",
        ),
        (
            "git@github.com-work:octo/widgets.git",
            "git@github.org:octo/widgets.git",
        ),
    ] {
        assert_eq!(
            github_repository(github, &resolver).unwrap(),
            Some(octo()),
            "{github}"
        );
        assert_eq!(
            github_repository(other, &resolver).unwrap(),
            None,
            "{other}"
        );
    }
}

#[test]
fn a_custom_ssh_command_resolves_no_alias_and_leaves_github_plain() {
    let refused = github_repository("git@work-gh:octo/widgets.git", &Resolver::Custom);
    assert_eq!(refused.unwrap_err().code, "github::remote::unknown");
    let plain = github_repository("git@github.com:octo/widgets.git", &Resolver::Custom);
    assert_eq!(plain.unwrap(), Some(octo()));
}

/// A host whose processes run for real, with a home of their own and no
/// system or global git config, except `ssh -G` when `aliases` names
/// hosts: it then prints the `hostname` the table gives, or fails.
struct Real {
    settings: Settings,
    home: tempfile::TempDir,
    aliases: &'static [(&'static str, &'static str)],
}

/// A host whose `ssh -G` resolves each host by `aliases` and no other.
fn aliased(aliases: &'static [(&'static str, &'static str)]) -> Real {
    Real { aliases, ..real() }
}

fn real() -> Real {
    let home = tempfile::tempdir().unwrap();
    let variables = [
        (
            "PATH",
            OsString::from("/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"),
        ),
        ("HOME", home.path().as_os_str().to_owned()),
        ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
    ];
    Real {
        settings: Settings {
            now: SystemTime::now(),
            day_start: None,
            caller: None,
            cache_home: None,
            environment: Variables(
                variables
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value))
                    .collect(),
            ),
        },
        home,
        aliases: &[],
    }
}

impl Real {
    /// Runs git in `at` with `arguments`, which must succeed.
    fn git(&self, at: &Path, arguments: &[&str]) {
        let arguments: Vec<String> = arguments.iter().map(|each| (*each).to_owned()).collect();
        let process = Process {
            program: "git",
            arguments: &arguments,
            directory: at,
            input: b"",
            deadline: Duration::from_secs(30),
        };
        let exited = start(&process, &self.settings.environment.0).unwrap();
        assert_eq!(
            exited.code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&exited.stderr)
        );
    }

    /// An empty git repository with an origin on GitHub.
    fn repository(&self) -> std::path::PathBuf {
        let at = self.home.path().join("widgets");
        let arguments = ["init", "-q", at.to_str().unwrap()];
        self.git(self.home.path(), &arguments);
        let origin = [
            "remote",
            "add",
            "origin",
            "https://github.com/Octo/Widgets.git",
        ];
        self.git(&at, &origin);
        at
    }
}

impl Host for Real {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("the URL rule reads no board")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("the URL rule reads no board")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("the URL rule reads no board")
    }

    fn put(
        &self,
        _lines: &[serde_json::Value],
    ) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("the URL rule writes no record")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &serde_json::Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("the URL rule calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("the URL rule opens no board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("the URL rule asks the owner nothing")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }

    fn run_hiding(&self, run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        if run.program == "ssh" && !self.aliases.is_empty() {
            let asked = run.arguments.last().map(String::as_str);
            let found = (self.aliases.iter()).find(|(alias, _)| Some(*alias) == asked);
            return Ok(Exited {
                code: Some(if found.is_some() { 0 } else { 255 }),
                stdout: found.map_or_else(Vec::new, |(_, name)| {
                    format!("hostname {name}\n").into_bytes()
                }),
                stderr: Vec::new(),
            });
        }
        let environment: Vec<_> = (self.settings.environment.0.iter())
            .filter(|(name, _)| !hidden.contains(&name.as_str()))
            .cloned()
            .collect();
        let process = Process {
            program: run.program,
            arguments: run.arguments,
            directory: run.directory.unwrap_or(self.home.path()),
            input: run.input,
            deadline: run.deadline,
        };
        start(&process, &environment)
    }
}

fn found(host: &Real, at: &Path, remote: &str, url: &str, via: Via) -> Vec<String> {
    destinations(host, at, remote, url, via).unwrap()
}

#[test]
fn a_push_to_a_github_url_goes_there_and_to_the_remote_as_configured() {
    let host = real();
    let at = host.repository();
    let other = "https://github.com/octo/other.git";
    assert_eq!(
        found(&host, &at, "origin", other, Via::Push),
        ["github:octo/other", "github:octo/widgets"]
    );
    assert_eq!(
        found(&host, &at, "origin", "/srv/widgets.git", Via::Push),
        ["github:octo/widgets"]
    );
}

#[test]
fn a_push_url_unlike_the_fetch_url_names_the_push_url_for_a_push_and_the_fetch_url_for_gh() {
    let host = real();
    let at = host.repository();
    let covered = [
        "remote",
        "set-url",
        "--push",
        "origin",
        "git@github.com:octo/covered.git",
    ];
    host.git(&at, &covered);
    assert_eq!(
        found(&host, &at, "origin", "/srv/covered.git", Via::Push),
        ["github:octo/covered"]
    );
    assert_eq!(
        found(&host, &at, "origin", "", Via::Gh),
        ["github:octo/widgets"]
    );
    assert_eq!(
        found(&host, &at, "origin", "", Via::Push),
        ["github:octo/covered"]
    );
}

#[test]
fn a_push_to_another_host_goes_to_no_github_repository_and_one_to_github_does() {
    let host = real();
    let at = host.repository();
    let elsewhere = "https://gitlab.example/o/w.git";
    host.git(&at, &["remote", "add", "elsewhere", elsewhere]);
    assert!(found(&host, &at, "elsewhere", elsewhere, Via::Push).is_empty());
    assert_eq!(
        found(&host, &at, "origin", "", Via::Push),
        ["github:octo/widgets"]
    );
}

#[test]
fn gits_own_ssh_command_makes_an_alias_unreadable_and_leaves_github_plain() {
    let host = real();
    let at = host.repository();
    assert!(matches!(resolver(&host, &at), Resolver::Ssh(_)));
    host.git(&at, &["config", "core.sshCommand", "ssh -F /dev/null"]);
    let custom = resolver(&host, &at);
    let refused = github_repository("git@work-gh:octo/widgets.git", &custom);
    assert_eq!(refused.unwrap_err().code, "github::remote::unknown");
    let plain = github_repository("git@github.com:octo/widgets.git", &custom);
    assert_eq!(plain.unwrap(), Some(octo()));
}

#[test]
fn only_a_plain_host_name_reaches_ssh() {
    let host = real();
    assert_eq!(ssh_hostname(&host, "a%h;b"), None);
    let refused = github_repository("git@a%h;b:octo/widgets.git", &Resolver::Ssh(&host));
    assert_eq!(refused.unwrap_err().code, "github::remote::unknown");
    assert_eq!(
        ssh_hostname(&host, "localhost").as_deref(),
        Some("localhost")
    );
}
