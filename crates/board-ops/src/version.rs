//! The Board versions a board's `min_board` compares
//! (docs/architecture/writes/minimum.md).

use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use board_tree::WriteGate;
use serde::{Deserialize, Deserializer};

/// A release number, `major.minor.patch`, and whether it is a pre-release.
/// A `-` suffix, such as `-rc1`, marks a pre-release, which is older than
/// the release it precedes; a `+` suffix, such as the `+<commit>` that
/// `board --version` appends, is read and ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    numbers: [u64; 3],
    /// False for a pre-release, which sorts it below the release.
    release: bool,
}

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let before_build = text.split('+').next().unwrap_or_default();
        let (numbers, pre) = match before_build.split_once('-') {
            Some((numbers, _)) => (numbers, true),
            None => (before_build, false),
        };
        let parts: Vec<&str> = numbers.split('.').collect();
        let number = |part: &str| {
            let plain = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            plain.then(|| part.parse().ok()).flatten()
        };
        match parts[..] {
            [major, minor, patch] => match (number(major), number(minor), number(patch)) {
                (Some(major), Some(minor), Some(patch)) => Ok(Self {
                    numbers: [major, minor, patch],
                    release: !pre,
                }),
                _ => Err(not_a_version(text)),
            },
            _ => Err(not_a_version(text)),
        }
    }
}

fn not_a_version(text: &str) -> String {
    format!("{text:?} is not a version: a version is major.minor.patch, such as 0.4.0")
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.numbers;
        let pre = if self.release { "" } else { "-pre" };
        write!(f, "{major}.{minor}.{patch}{pre}")
    }
}

/// When a build's source was committed, in whole seconds since the Unix
/// epoch. In a config it is an RFC 3339 timestamp such as
/// `2026-11-13T10:00:00Z`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Built(i64);

impl Built {
    pub(crate) fn from_seconds(seconds: i64) -> Self {
        Self(seconds)
    }
}

impl FromStr for Built {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let at: jiff::Timestamp = text
            .parse()
            .map_err(|_| format!("{text:?} is not an RFC 3339 timestamp"))?;
        Ok(Self(at.as_second()))
    }
}

impl fmt::Display for Built {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match jiff::Timestamp::from_second(self.0) {
            Ok(at) => write!(f, "{at}"),
            Err(_) => write!(f, "{}", self.0),
        }
    }
}

/// The Board that is running: its version, the `+<commit>` of the source it
/// was built from when it was built from one, and the date of that commit
/// when the build embedded one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Running {
    pub(crate) version: Version,
    /// What follows the `+` of the build text, such as `45aafa9`.
    pub(crate) source: Option<String>,
    pub(crate) built: Option<Built>,
}

impl Running {
    /// The build `text` names, such as `0.0.0+45aafa9`, built at `built`
    /// seconds since the Unix epoch.
    pub(crate) fn parse(text: &str, built: Option<i64>) -> Result<Self, String> {
        Ok(Self {
            version: text.parse()?,
            source: text.split_once('+').map(|(_, source)| source.to_owned()),
            built: built.map(Built::from_seconds),
        })
    }

    /// Whether this is a build from a source checkout, which no release
    /// replaces: one that names its commit, or is numbered 0.0.0.
    fn is_source(&self) -> bool {
        self.source.is_some() || self.version == Version::default()
    }
}

impl fmt::Display for Running {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Board {}", self.version)?;
        if let Some(source) = &self.source {
            write!(f, "+{source}")?;
        }
        match self.built {
            Some(built) => write!(f, " (built {built})"),
            None => write!(f, " (no build date)"),
        }
    }
}

/// The build a binary declared with [`declare_build`].
static DECLARED: OnceLock<Running> = OnceLock::new();

/// Names the Board this process is, once: its build text, such as
/// `0.0.0+45aafa9`, and the commit date of its source in seconds since the
/// Unix epoch when the build embedded one. Every board opened after this
/// gates its writes as that build, whichever code opens it, so a binary
/// declares itself in one place and no caller carries the build to a board it
/// opens. Later declarations and text that is no version change nothing; a
/// process that never declares runs as this crate's version with no date.
pub fn declare_build(text: &str, committed: Option<i64>) {
    if let Ok(running) = Running::parse(text, committed) {
        let _ = DECLARED.set(running);
    }
}

/// The build boards are opened as: the declared one, or else this crate's
/// version with no date.
pub(crate) fn declared() -> Running {
    DECLARED.get().cloned().unwrap_or_else(|| Running {
        version: Version::from_str(env!("CARGO_PKG_VERSION")).unwrap_or_default(),
        source: None,
        built: None,
    })
}

/// A board's `min_board`: the oldest Board that may write it. It is a
/// version string (`"0.4.0"`), a UTC timestamp (`"2026-11-13T10:00:00Z"`),
/// or an object with `version` and `built`, either or both. A Board older
/// on a count that is named is refused; a build with no date is judged by
/// the version alone. Any other value, such as a key or a string form a
/// newer Board introduced, is one this Board cannot read, so it refuses to
/// write the board and still reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinBoard(Reading);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Reading {
    Named {
        version: Option<Version>,
        built: Option<Built>,
    },
    Unreadable(String),
}

impl MinBoard {
    /// What this `min_board` makes of writes by `running`, if anything.
    pub(crate) fn gate(&self, running: &Running) -> Option<WriteGate> {
        let (version, built) = match &self.0 {
            Reading::Unreadable(detail) => {
                return Some(WriteGate::Unreadable {
                    detail: detail.clone(),
                });
            }
            Reading::Named { version, built } => (version, built),
        };
        let old_version = version.filter(|min| running.version < *min);
        let old_build = match (built, running.built) {
            (Some(min), Some(built)) if built < *min => Some(*min),
            _ => None,
        };
        if old_version.is_some() || old_build.is_some() {
            return Some(WriteGate::TooOld {
                min_board: self.to_string(),
                running: running.to_string(),
                install: install(running, old_version, old_build),
            });
        }
        match (built, running.built) {
            (Some(min), None) => Some(WriteGate::Unchecked {
                note: format!(
                    "this Board has no build date, so it cannot check the build date {min} that this board's `min_board` names, and it wrote the board anyway; build it from a git checkout, or set SOURCE_DATE_EPOCH or BOARD_BUILD_COMMIT, so that it embeds one"
                ),
            }),
            _ => None,
        }
    }
}

/// What to install so that `running` satisfies the counts `min_board` names
/// that it is older on.
fn install(running: &Running, version: Option<Version>, built: Option<Built>) -> String {
    if running.is_source() {
        let mut steps = Vec::new();
        if let Some(version) = version {
            steps.push(format!("check out tag v{version} or a later commit"));
        }
        if let Some(built) = built {
            steps.push(format!(
                "check out a commit dated {built} or later (`git pull` on an older branch is not enough)"
            ));
        }
        format!(
            "{}, then build and install it with `cargo install --path crates/board-cli --locked`",
            steps.join(", and ")
        )
    } else {
        let mut steps = Vec::new();
        if let Some(version) = version {
            steps.push(format!(
                "install a release of version {version} or newer (README.md, \"Installing a release\")"
            ));
        }
        if let Some(built) = built {
            steps.push(format!(
                "install a release built from source committed {built} or later"
            ));
        }
        steps.join(", and ")
    }
}

impl fmt::Display for MinBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Reading::Named { version, built } = &self.0 else {
            return write!(f, "a `min_board` this Board cannot read");
        };
        let mut parts = Vec::new();
        if let Some(version) = version {
            parts.push(format!("version {version} or newer"));
        }
        if let Some(built) = built {
            parts.push(format!("a build dated {built} or later"));
        }
        write!(f, "{}", parts.join(" and "))
    }
}

impl<'de> Deserialize<'de> for MinBoard {
    /// Never fails: a value this Board cannot read is kept as such, so that
    /// the board it is in still opens for reads.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self(match read(&value) {
            Ok((version, built)) => Reading::Named { version, built },
            Err(detail) => Reading::Unreadable(detail),
        }))
    }
}

/// The version and the build date `value` names, or why it names neither.
fn read(value: &serde_json::Value) -> Result<(Option<Version>, Option<Built>), String> {
    use serde_json::Value;
    match value {
        Value::String(text) => match (text.parse::<Version>(), text.parse::<Built>()) {
            (Ok(version), _) => Ok((Some(version), None)),
            (_, Ok(built)) => Ok((None, Some(built))),
            _ => Err(format!(
                "{text:?} is neither a version (major.minor.patch, such as 0.4.0) nor an RFC 3339 UTC timestamp (such as 2026-11-13T00:00:00Z)"
            )),
        },
        Value::Object(pair) => {
            if let Some(key) = pair
                .keys()
                .find(|key| !matches!(key.as_str(), "version" | "built"))
            {
                return Err(format!(
                    "the key {key:?} is not one of `version` and `built`"
                ));
            }
            let text = |key: &str| match pair.get(key) {
                None => Ok(None),
                Some(Value::String(text)) => Ok(Some(text)),
                Some(_) => Err(format!("`{key}` is not a string")),
            };
            let version = text("version")?.map(|text| text.parse()).transpose()?;
            let built = text("built")?.map(|text| text.parse()).transpose()?;
            if version.is_none() && built.is_none() {
                return Err("it names neither a version nor a build date".to_owned());
            }
            Ok((version, built))
        }
        _ => Err("it is neither a string nor an object".to_owned()),
    }
}

#[cfg(test)]
mod tests;
