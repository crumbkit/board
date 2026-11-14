//! The Board versions a board's `min_board` compares
//! (docs/architecture/writes/minimum.md).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// A release number, `major.minor.patch`. A `-` or `+` suffix, such as the
/// `+<commit>` that `board --version` appends, is read and ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version([u64; 3]);

impl FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let numbers = text.split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<&str> = numbers.split('.').collect();
        let number = |part: &str| {
            let plain = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            plain.then(|| part.parse().ok()).flatten()
        };
        match parts[..] {
            [major, minor, patch] => match (number(major), number(minor), number(patch)) {
                (Some(major), Some(minor), Some(patch)) => Ok(Self([major, minor, patch])),
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
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
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

/// The Board that is running: its version, and the date of its source's
/// commit when the build embedded one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Running {
    pub(crate) version: Version,
    pub(crate) built: Option<Built>,
}

impl fmt::Display for Running {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.built {
            Some(built) => write!(f, "Board {} (built {built})", self.version),
            None => write!(f, "Board {} (no build date)", self.version),
        }
    }
}

/// A board's `min_board`: the oldest Board that may write it. It is a
/// version string (`"0.4.0"`), a UTC timestamp (`"2026-11-13T10:00:00Z"`),
/// or an object with `version` and `built`, either or both. A Board older
/// on a count that is named is refused; a build with no date is judged by
/// the version alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinBoard {
    version: Option<Version>,
    built: Option<Built>,
}

impl MinBoard {
    /// Whether a board that names this refuses writes from `running`.
    pub(crate) fn refuses(&self, running: &Running) -> bool {
        let old_version = self.version.is_some_and(|min| running.version < min);
        let old_build =
            matches!((self.built, running.built), (Some(min), Some(built)) if built < min);
        old_version || old_build
    }
}

impl fmt::Display for MinBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if let Some(version) = self.version {
            parts.push(format!("version {version} or newer"));
        }
        if let Some(built) = self.built {
            parts.push(format!("a build dated {built} or later"));
        }
        write!(f, "{}", parts.join(" and "))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Shape {
    Text(String),
    Both(Pair),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    built: Option<String>,
}

impl<'de> Deserialize<'de> for MinBoard {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let (version, built) = match Shape::deserialize(deserializer)? {
            Shape::Text(text) => match text.parse::<Version>() {
                Ok(version) => (Some(version), None),
                Err(_) => match text.parse::<Built>() {
                    Ok(built) => (None, Some(built)),
                    Err(_) => {
                        return Err(Error::custom(format!(
                            "{text:?} is neither a version (major.minor.patch, such as 0.4.0) nor an RFC 3339 UTC timestamp (such as 2026-11-13T00:00:00Z)"
                        )));
                    }
                },
            },
            Shape::Both(Pair { version, built }) => (
                version
                    .map(|text| text.parse())
                    .transpose()
                    .map_err(Error::custom)?,
                built
                    .map(|text| text.parse())
                    .transpose()
                    .map_err(Error::custom)?,
            ),
        };
        if version.is_none() && built.is_none() {
            return Err(Error::custom(
                "min_board names neither a version nor a build date",
            ));
        }
        Ok(Self { version, built })
    }
}

#[cfg(test)]
mod tests;
