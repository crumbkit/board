//! `lint.toml`: the numbers this lint enforces.

use serde::Deserialize;

use crate::files::{FilesError, Root};

pub(crate) const FILE: &str = "lint.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(rename = "$type")]
    _type: ConfigType,
    pub(crate) context: ContextBudget,
    pub(crate) docs_index: FileBudget,
    pub(crate) docs: GlobBudget,
    pub(crate) plan: FileBudget,
    pub(crate) source: SourceBudget,
    pub(crate) expect: ExpectBudget,
}

#[derive(Debug, Deserialize)]
enum ConfigType {
    #[serde(rename = "board/lint-config/v1")]
    V1,
}

/// Files an agent loads at session start, and the most words they may hold
/// together with everything they import.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextBudget {
    pub(crate) entry: Vec<String>,
    pub(crate) max_words: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileBudget {
    pub(crate) file: String,
    pub(crate) max_words: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobBudget {
    pub(crate) glob: String,
    pub(crate) max_words: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceBudget {
    pub(crate) max_lines: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectBudget {
    pub(crate) count: usize,
}

impl Config {
    /// Each section with a `max_words`: its name, the files or glob it
    /// names, and the limit.
    pub(crate) fn word_budgets(&self) -> [(&'static str, Vec<&str>, usize); 4] {
        let context = self.context.entry.iter().map(String::as_str).collect();
        let index = &self.docs_index;
        [
            ("context", context, self.context.max_words),
            ("docs_index", vec![&index.file], index.max_words),
            ("docs", vec![&self.docs.glob], self.docs.max_words),
            ("plan", vec![&self.plan.file], self.plan.max_words),
        ]
    }

    pub(crate) fn load(root: &Root) -> Result<Self, ConfigError> {
        Self::parse(&root.read(FILE)?)
    }

    pub(crate) fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Invalid { source })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error(transparent)]
    Files(#[from] FilesError),
    #[error("lint.toml is not a valid board/lint-config/v1 document")]
    Invalid {
        #[source]
        source: toml::de::Error,
    },
}

impl ConfigError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Files(error) => error.code(),
            Self::Invalid { .. } => "xtask::config::invalid",
        }
    }
}

#[cfg(test)]
mod tests;
