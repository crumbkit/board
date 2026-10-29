//! `lint.toml`: the numbers this lint enforces.

use serde::Deserialize;

use crate::files::{FilesError, Root};
use crate::signing::SigningConfig;

pub(crate) const FILE: &str = "lint.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(rename = "$type")]
    _type: ConfigType,
    pub(crate) context: ContextBudget,
    pub(crate) docs_index: FileBudget,
    pub(crate) docs: DirectoryBudget,
    pub(crate) plan: FileBudget,
    pub(crate) plan_leaves: DirectoryBudget,
    pub(crate) source: SourceBudget,
    pub(crate) expect: ExpectBudget,
    pub(crate) signing: SigningConfig,
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
pub(crate) struct DirectoryBudget {
    pub(crate) directory: String,
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
    /// Each section with a `max_words`: its name, the files or GLOB it
    /// names, and the limit. A directory's GLOB matches every `.md` file below
    /// it, because `*` matches `/` in `SQLite` GLOB.
    pub(crate) fn word_budgets(&self) -> [(&'static str, Vec<String>, usize); 5] {
        let context = self.context.entry.clone();
        let index = &self.docs_index;
        [
            ("context", context, self.context.max_words),
            ("docs_index", vec![index.file.clone()], index.max_words),
            (
                "docs",
                vec![format!("{}/*.md", self.docs.directory)],
                self.docs.max_words,
            ),
            ("plan", vec![self.plan.file.clone()], self.plan.max_words),
            (
                "plan_leaves",
                vec![format!("{}/*.md", self.plan_leaves.directory)],
                self.plan_leaves.max_words,
            ),
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
