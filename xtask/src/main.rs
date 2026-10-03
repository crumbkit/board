//! Repository lints that clippy cannot express: the documentation budgets, the
//! source file length cap and the `#[expect]` ratchet. `bin/lint` runs it as
//! `cargo xtask lint`; `lint.toml` holds every number it enforces.

mod budgets;
mod config;
mod files;
mod report;
mod source;
mod words;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::process::ExitCode;

use crate::config::{Config, ConfigError};
use crate::files::{FilesError, Root};
use crate::report::{Output, Report};

/// The workspace root: this crate's manifest directory sits one level below it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() != Some("lint") || arguments.next().is_some() {
        report::emit(&Output::Usage);
        return ExitCode::from(2);
    }
    match lint(Path::new(ROOT)) {
        Ok(report) => {
            report::emit(&Output::Report(&report));
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            report::emit(&Output::Failure {
                code: error.code(),
                error: &error,
            });
            ExitCode::FAILURE
        }
    }
}

/// Runs every check against the repository at `root`.
fn lint(root: &Path) -> Result<Report, LintError> {
    let root = Root::open(root)?;
    let config = Config::load(&root)?;
    let [lines, expects] = source::checks(&root, &config.source, &config.expect)?;
    Ok(Report {
        checks: vec![
            budgets::context(&root, &config.context)?,
            budgets::docs_index(&root, &config.docs_index)?,
            budgets::docs(&root, &config.docs)?,
            lines,
            expects,
        ],
    })
}

#[derive(Debug, thiserror::Error)]
enum LintError {
    #[error(transparent)]
    Files(#[from] FilesError),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl LintError {
    fn code(&self) -> &'static str {
        match self {
            Self::Files(error) => error.code(),
            Self::Config(error) => error.code(),
        }
    }
}
