//! Repository lints that clippy cannot express: the word budgets, the
//! source file length cap, the `#[expect]` ratchet, the signing literals'
//! owners, and the budget controls' agreement with lint.toml. `bin/lint` runs it as
//! `cargo xtask lint`; `lint.toml` holds every number it enforces.

mod bench;
mod budgets;
mod config;
mod controls;
mod files;
mod report;
mod signing;
mod source;
mod words;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::process::ExitCode;

use crate::config::{Config, ConfigError};
use crate::files::{FilesError, Root};
use crate::report::{CheckKind, Output, Report};

/// The workspace root: this crate's manifest directory sits one level below it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("lint") if arguments.next().is_none() => {}
        Some("bench") => {
            let Some(options) = bench::Options::parse(arguments) else {
                report::emit(&Output::Usage);
                return ExitCode::from(2);
            };
            return match bench(Path::new(ROOT), &options) {
                Ok(timed) => {
                    report::emit(&Output::Bench(&timed));
                    if timed.iter().all(bench::Timed::passed) {
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
            };
        }
        _ => {
            report::emit(&Output::Usage);
            return ExitCode::from(2);
        }
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
    let mut checks = vec![
        budgets::context(&root, &config.context)?,
        budgets::file(&root, CheckKind::DocsIndex, &config.docs_index)?,
    ];
    checks.extend(budgets::docs(&root, &config.docs)?);
    checks.extend([budgets::file(&root, CheckKind::Plan, &config.plan)?]);
    checks.extend(budgets::plan_leaves(
        &root,
        &config.plan.file,
        &config.plan_leaves,
    )?);
    checks.push(lines);
    checks.push(expects);
    checks.push(signing::check(&root, &config.signing)?);
    Ok(Report {
        checks,
        controls: controls::check(&root, &config)?,
    })
}

/// Times the budgeted operations on the board `options` names, or on one
/// generated for the run.
/// The plugin call gets this machine's `PATH` and `HOME`, the `board` beside
/// this binary, and a data directory that installs the repository's tasks
/// plugin.
#[expect(
    clippy::disallowed_methods,
    reason = "bench reads PATH and HOME, and installs the plugin it calls"
)]
fn bench(root: &Path, options: &bench::Options) -> Result<Vec<bench::Timed>, bench::BenchError> {
    let docs = std::fs::read_to_string(root.join("docs/architecture/performance/budgets.md"))
        .map_err(bench::BenchError::Write)?;
    let budgets = bench::budgets(&docs)?;
    let data = tempfile::tempdir().map_err(bench::BenchError::Write)?;
    let plugins = data.path().join("board/plugins");
    std::fs::create_dir_all(&plugins).map_err(bench::BenchError::Write)?;
    std::os::unix::fs::symlink(root.join("plugins/tasks"), plugins.join("tasks"))
        .map_err(bench::BenchError::Write)?;
    let exe = std::env::current_exe().map_err(bench::BenchError::Write)?;
    let host = bench::Host {
        path: std::env::var_os("PATH").unwrap_or_default(),
        home: std::env::var_os("HOME"),
        board: exe.parent().map(Path::to_path_buf).unwrap_or_default(),
        data: data.path().to_owned(),
    };
    if let Some(board) = &options.board {
        return bench::run(board, &budgets, &host, bench::Corpus::Given);
    }
    let generated = tempfile::tempdir().map_err(bench::BenchError::Write)?;
    bench::generate(generated.path(), options.records).map_err(bench::BenchError::Write)?;
    bench::run(generated.path(), &budgets, &host, bench::Corpus::Generated)
}

#[derive(Debug, thiserror::Error)]
enum LintError {
    #[error(transparent)]
    Files(#[from] FilesError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{path} is not a control with a requirement; see controls/control/v1")]
    Control {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

impl LintError {
    fn code(&self) -> &'static str {
        match self {
            Self::Files(error) => error.code(),
            Self::Config(error) => error.code(),
            Self::Control { .. } => "xtask::control::invalid",
        }
    }
}
