//! The launchd agent that runs `board --personal autosync run` on an
//! interval and at load: `install` writes its plist to a temporary name,
//! renames it into place and bootstraps it; `uninstall` boots it out and
//! deletes the plist.

use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, PluginError, Run};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{home, refusal, state};

/// The agent's launchd label, and its plist's name.
pub(crate) const LABEL: &str = "board.autosync";

/// The interval's default and bounds, in seconds.
const INTERVAL: u64 = 300;
const SHORTEST: u64 = 60;
const LONGEST: u64 = 3600;

/// What `install` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "InstallInput")]
pub(crate) struct InstallInput {
    /// Seconds between runs, 60 to 3,600; 300 when left out.
    #[schemars(range(min = 60, max = 3600))]
    pub(crate) interval: Option<u64>,
    /// Prints the plist and loads nothing.
    #[serde(default, rename = "dry-run")]
    pub(crate) dry_run: bool,
}

/// What `uninstall` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "UninstallInput")]
pub(crate) struct UninstallInput {
    /// Says what it would remove and removes nothing.
    #[serde(default, rename = "dry-run")]
    pub(crate) dry_run: bool,
}

/// What `install` and `uninstall` did.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "AgentOutput", deny_unknown_fields)]
pub(crate) struct Output {
    /// The plist's path.
    pub(crate) plist: String,
    /// The plist's text; empty for `uninstall`.
    pub(crate) text: String,
    /// Whether launchd was told, false for a dry run.
    pub(crate) loaded: bool,
}

/// `~/Library/LaunchAgents/board.autosync.plist`.
fn plist(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

/// The plist that runs `board` every `interval` seconds and at load, with
/// `path` as its `PATH`, so it finds git and gitleaks as this run did.
pub(crate) fn text(board: &str, interval: u64, path: &str, home: &str, logs: &Path) -> String {
    let escaped = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let logs = logs.display().to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
		<string>--personal</string>
		<string>autosync</string>
		<string>run</string>
	</array>
	<key>StartInterval</key>
	<integer>{interval}</integer>
	<key>RunAtLoad</key>
	<true/>
	<key>EnvironmentVariables</key>
	<dict>
		<key>PATH</key>
		<string>{}</string>
		<key>HOME</key>
		<string>{}</string>
	</dict>
	<key>StandardOutPath</key>
	<string>{}/agent.out</string>
	<key>StandardErrorPath</key>
	<string>{}/agent.err</string>
</dict>
</plist>
"#,
        escaped(board),
        escaped(path),
        escaped(home),
        escaped(&logs),
        escaped(&logs),
    )
}

/// The launchd domain of this user's agents.
fn domain() -> String {
    format!("gui/{}", rustix::process::getuid().as_raw())
}

fn launchctl(host: &dyn Host, arguments: &[&str]) -> Result<i32, String> {
    let arguments: Vec<String> = arguments.iter().map(|&word| word.to_owned()).collect();
    let exited = host
        .run(&Run {
            program: "launchctl",
            arguments: &arguments,
            directory: None,
            input: &[],
            deadline: Duration::from_secs(30),
            added: &[],
        })
        .map_err(|error| format!("launchctl did not run: {error}"))?;
    Ok(exited.code.unwrap_or(-1))
}

/// Writes and loads the agent, or with `dry_run` prints its plist.
pub(crate) fn install(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: InstallInput = PluginError::read_input("autosync", "install", input)?;
    let interval = given.interval.unwrap_or(INTERVAL);
    if !(SHORTEST..=LONGEST).contains(&interval) {
        return Err(refusal(
            "autosync::agent::interval",
            format!("the interval {interval} is not 60 to 3,600 seconds"),
            Map::new(),
            "give --interval between 60 and 3600, or leave it out for 300",
        ));
    }
    let home = home(host)?;
    let variables = &host.settings().environment;
    let path = variables
        .get("PATH")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let board = std::env::current_exe()
        .map_or_else(|_| "board".to_owned(), |board| board.display().to_string());
    let file = plist(&home);
    let written = text(
        &board,
        interval,
        &path,
        &home.display().to_string(),
        &state(&home),
    );
    let output = |loaded| Output {
        plist: file.display().to_string(),
        text: written.clone(),
        loaded,
    };
    if given.dry_run {
        return Ok(json!(output(false)));
    }
    let failed = |why: String| {
        refusal(
            "autosync::agent::failed",
            format!("the agent was not installed: {why}"),
            Map::new(),
            "run `board --personal autosync install` again; `--dry-run` shows the plist",
        )
    };
    let target = format!("{}/{LABEL}", domain());
    // A loaded agent is booted out first, so the new plist is the one read.
    launchctl(host, &["bootout", &target]).map_err(failed)?;
    board_index::replace_file(&file, written.as_bytes(), 0o644)
        .map_err(|error| failed(format!("{} cannot be written: {error}", file.display())))?;
    let file_text = file.display().to_string();
    match launchctl(host, &["bootstrap", &domain(), &file_text]).map_err(failed)? {
        0 => Ok(json!(output(true))),
        code => Err(failed(format!("launchctl bootstrap exited with {code}"))),
    }
}

/// Unloads the agent and deletes its plist.
pub(crate) fn uninstall(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: UninstallInput = PluginError::read_input("autosync", "uninstall", input)?;
    let file = plist(&home(host)?);
    let output = |loaded| Output {
        plist: file.display().to_string(),
        text: String::new(),
        loaded,
    };
    if given.dry_run {
        return Ok(json!(output(false)));
    }
    let failed = |why: String| {
        refusal(
            "autosync::agent::failed",
            format!("the agent was not uninstalled: {why}"),
            Map::new(),
            "run `board --personal autosync uninstall` again",
        )
    };
    launchctl(host, &["bootout", &format!("{}/{LABEL}", domain())]).map_err(failed)?;
    board_index::remove_state(&file)
        .map_err(|error| failed(format!("{} cannot be removed: {error}", file.display())))?;
    Ok(json!(output(true)))
}
