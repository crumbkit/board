//! The gitleaks scan of exactly the commits a push sends, and the checks
//! around it: that gitleaks is new enough, that the board holds no gitleaks
//! file that could let a secret through, and the Mac's fingerprints file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use board_ops::{Host, Pushing, Run};
use board_plugin::child::ProcessError;
use serde::Serialize;

use crate::origin::git;

/// The first gitleaks release with `gitleaks git`.
const OLDEST: (u64, u64, u64) = (8, 19, 0);

/// The files gitleaks reads from the root of the repository it scans,
/// either of which could let a secret through.
pub(crate) const BOARD_CONFIG: [&str; 2] = [".gitleaks.toml", ".gitleaksignore"];

/// The variables that would point gitleaks at another config.
const HIDDEN: [&str; 2] = ["GITLEAKS_CONFIG", "GITLEAKS_CONFIG_TOML"];

/// The largest report read, and the largest fingerprints file.
const MAX_REPORT: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_FINGERPRINTS: u64 = 1024 * 1024;

/// The exit code gitleaks is told to give for a finding.
const FOUND: i32 = 3;

/// Whether gitleaks can scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Scanner {
    Ready,
    Missing,
    TooOld { version: String },
}

/// `gitleaks version` on the host's PATH, against [`OLDEST`].
pub(crate) fn scanner(host: &dyn Host) -> Scanner {
    let ran = host.run(&Run {
        program: "gitleaks",
        arguments: &["version".to_owned()],
        directory: None,
        input: &[],
        deadline: Duration::from_secs(30),
        added: &[],
    });
    let Ok(exited) = ran else {
        return Scanner::Missing;
    };
    let printed = String::from_utf8_lossy(&exited.stdout).trim().to_owned();
    match version(&printed) {
        Some(found) if exited.code == Some(0) && found >= OLDEST => Scanner::Ready,
        _ if exited.code != Some(0) => Scanner::Missing,
        _ => Scanner::TooOld { version: printed },
    }
}

/// `8.30.1`, or `v8.30.1`, as three numbers.
fn version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim_start_matches('v').splitn(3, '.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    Some((next()?, next()?, next()?))
}

/// The Mac's fingerprints file, when it exists, or why it cannot be used.
pub(crate) fn fingerprints(file: &Path) -> Result<Option<PathBuf>, u64> {
    match std::fs::metadata(file) {
        Err(_) => Ok(None),
        Ok(held) if held.len() > MAX_FINGERPRINTS => Err(held.len()),
        Ok(_) => Ok(Some(file.to_path_buf())),
    }
}

/// The first gitleaks file the board holds, in the tree at `new` or in the
/// working tree.
pub(crate) fn board_config(host: &dyn Host, board: &Path, new: &str) -> Option<&'static str> {
    BOARD_CONFIG.into_iter().find(|name| {
        let in_tree = git(host, board, &["cat-file", "-e", &format!("{new}:{name}")])
            .is_some_and(|ran| ran.ok());
        in_tree || board.join(name).symlink_metadata().is_ok()
    })
}

/// What a finding keeps: never the secret or its line's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct Finding {
    pub(crate) rule: String,
    /// The commit's first 12 hex digits.
    pub(crate) commit: String,
    pub(crate) file: String,
    pub(crate) line: u64,
}

/// What a scan found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Scanned {
    Clean,
    Found(Vec<Finding>),
    Failed(ScanFailure),
}

/// Why a scan said nothing about the commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanFailure {
    NotStarted,
    TimedOut,
    Exited(Option<i32>),
    Unreadable,
}

impl ScanFailure {
    pub(crate) fn describe(self) -> String {
        match self {
            Self::NotStarted => "gitleaks did not start".to_owned(),
            Self::TimedOut => "gitleaks ran past its deadline".to_owned(),
            Self::Exited(Some(code)) => format!("gitleaks exited with {code}"),
            Self::Exited(None) => "gitleaks was stopped by a signal".to_owned(),
            Self::Unreadable => "gitleaks's report could not be read".to_owned(),
        }
    }
}

/// How a scan runs: where its report goes, the Mac's fingerprints, and
/// how long it may take.
#[derive(Clone, Debug)]
pub(crate) struct Scan {
    pub(crate) state: PathBuf,
    pub(crate) fingerprints: Option<PathBuf>,
    pub(crate) deadline: Duration,
}

/// The directory name each scan's report directory starts with, which the
/// next run removes if a killed run left one.
pub(crate) const SCRATCH: &str = "scan-";

impl Scan {
    /// Scans exactly the commits `pushing` sends from `board` with gitleaks
    /// at nice 10, then deletes the report.
    pub(crate) fn run(&self, host: &dyn Host, board: &Path, pushing: &Pushing) -> Scanned {
        let Ok(scratch) = board_index::scratch_directory(&self.state, SCRATCH) else {
            return Scanned::Failed(ScanFailure::Unreadable);
        };
        let report = scratch.path().join("report.json");
        let range = match pushing.fetched {
            Some(fetched) => format!("{fetched}..{}", pushing.new),
            None => pushing.new.to_string(),
        };
        let mut arguments: Vec<String> = [
            "-n",
            "10",
            "gitleaks",
            "git",
            "--no-banner",
            "--redact",
            "--exit-code",
            "3",
            "--log-opts",
            &range,
        ]
        .map(str::to_owned)
        .to_vec();
        if let Some(fingerprints) = &self.fingerprints {
            arguments.push("--gitleaks-ignore-path".to_owned());
            arguments.push(fingerprints.display().to_string());
        }
        arguments.extend(
            [
                "--report-format",
                "json",
                "--report-path",
                &report.display().to_string(),
                &board.display().to_string(),
            ]
            .map(str::to_owned),
        );
        let ran = host.run_hiding(
            &Run {
                program: "nice",
                arguments: &arguments,
                directory: Some(&self.state),
                input: &[],
                deadline: self.deadline,
                added: &[],
            },
            &HIDDEN,
        );
        let scanned = match ran {
            Err(ProcessError::NotStarted { .. }) => Scanned::Failed(ScanFailure::NotStarted),
            Err(_) => Scanned::Failed(ScanFailure::TimedOut),
            Ok(exited) if exited.code == Some(0) => Scanned::Clean,
            Ok(exited) if exited.code == Some(FOUND) => match read_report(&report) {
                Some(found) if !found.is_empty() => Scanned::Found(found),
                _ => Scanned::Failed(ScanFailure::Unreadable),
            },
            Ok(exited) => Scanned::Failed(ScanFailure::Exited(exited.code)),
        };
        drop(scratch);
        scanned
    }
}

/// The kept fields of each finding in gitleaks's JSON report, or `None`
/// when it is missing, too large or not a list of findings.
fn read_report(report: &Path) -> Option<Vec<Finding>> {
    if std::fs::metadata(report).ok()?.len() > MAX_REPORT {
        return None;
    }
    let bytes = std::fs::read(report).ok()?;
    let parsed = board_model::parse_json(&bytes).ok()?;
    let found = parsed.as_array()?;
    (found.iter())
        .map(|finding| {
            let text = |key: &str| finding.get(key)?.as_str().map(str::to_owned);
            Some(Finding {
                rule: text("RuleID")?,
                commit: text("Commit")?.chars().take(12).collect(),
                file: text("File")?,
                line: finding.get("StartLine")?.as_u64()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn a_version_reads_with_or_without_its_v_and_orders_by_number() {
        assert_eq!(version("8.30.1"), Some((8, 30, 1)));
        assert_eq!(version("v8.19.0"), Some((8, 19, 0)));
        assert!(version("8.18.4") < Some(super::OLDEST));
        assert!(version("8.19.0") >= Some(super::OLDEST));
        assert_eq!(version("dev"), None);
    }
}
