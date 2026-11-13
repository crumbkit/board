//! The Board versions a board's `min_board` compares
//! (docs/architecture/writes.md, Minimum version).

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// A release number, `major.minor.patch`. A `-` or `+` suffix, such as the
/// `+<commit>` that `board --version` appends, is read and ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version([u64; 3]);

impl Version {
    /// The number every build made from source carries until a release
    /// stamps its own.
    const UNRELEASED: Self = Self([0, 0, 0]);

    /// Whether a board that names `minimum` refuses writes from this
    /// version. An unreleased build, `0.0.0`, is never refused: it is
    /// built from source, and nothing says which release it follows.
    pub(crate) fn too_old_for(self, minimum: Self) -> bool {
        self != Self::UNRELEASED && self.cmp(&minimum) == Ordering::Less
    }
}

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
                _ => Err(format!("{text:?} is not major.minor.patch")),
            },
            _ => Err(format!("{text:?} is not major.minor.patch")),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
