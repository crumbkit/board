//! Repository lints that clippy cannot express: the word budgets, the
//! source file length cap, the `#[expect]` ratchet, the signing literals'
//! owners, and the budget controls' agreement with lint.toml. `bin/lint` runs it as
//! `cargo xtask lint`; `lint.toml` holds every number it enforces.

mod bench;
mod budgets;
mod config;
mod controls;
mod files;
mod process;
mod profile;
mod report;
mod signing;
mod source;
mod words;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::process::ExitCode;

use crate::bench::against;
use crate::config::{Config, ConfigError};
use crate::files::{FilesError, Root};
use crate::report::{CheckKind, Output, Report};

/// The workspace root: this crate's manifest directory sits one level below it.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let root = Path::new(ROOT);
    let usage = || {
        report::emit(&Output::Usage);
        ExitCode::from(2)
    };
    match arguments.next().as_deref() {
        Some("lint") if arguments.next().is_none() => {
            let linted = lint(root).map(|report| {
                report::emit(&Output::Report(&report));
                report.passed()
            });
            finish(linted, LintError::code)
        }
        Some("bench") => match bench::Options::parse(arguments) {
            Some(options) => finish(bench(root, &options), bench::BenchError::code),
            None => usage(),
        },
        Some("profile") => match profile::Profile::parse(arguments) {
            Some(profile) => finish(
                profile::run(root, &profile).map(|()| true),
                profile::ProfileError::code,
            ),
            None => usage(),
        },
        _ => usage(),
    }
}

/// Exit 0 when `result` passed, else 1, printing the error it failed with.
fn finish<E: std::error::Error>(
    result: Result<bool, E>,
    code: impl Fn(&E) -> &'static str,
) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            report::emit(&Output::Failure {
                code: code(&error),
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
/// generated for the run, or compares them with an older binary, and
/// prints whether they passed.
/// The plugin call gets this machine's `PATH` and `HOME`, the `board` beside
/// this binary, and a data directory that installs the repository's tasks
/// plugin.
#[expect(
    clippy::disallowed_methods,
    reason = "bench reads PATH and HOME, and installs the plugin it calls"
)]
fn bench(root: &Path, options: &bench::Options) -> Result<bool, bench::BenchError> {
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
    let base = options.against.as_deref().map(against::base).transpose()?;
    let generated = tempfile::tempdir().map_err(bench::BenchError::Write)?;
    let mounts = tempfile::tempdir().map_err(bench::BenchError::Write)?;
    let mounted;
    let (board, corpus) = if let Some(board) = &options.board {
        (board.as_path(), bench::Corpus::Given)
    } else {
        bench::generate(generated.path(), options.records, &[], 0)
            .map_err(bench::BenchError::Write)?;
        mounted = bench::generate_mounted(mounts.path(), options.records)
            .map_err(bench::BenchError::Write)?;
        bench::settle();
        let corpus = bench::Corpus::Generated { mounted: &mounted };
        (generated.path(), corpus)
    };
    let Some(base) = base else {
        let timed = bench::run(board, &budgets, &host, corpus)?;
        report::emit(&Output::Bench(&timed));
        return Ok(timed.iter().all(bench::Timed::passed));
    };
    let new = bench::plugin_path(&host).map(|_| host.board.join("board"))?;
    let (sample, directory) = bench::sample(&bench::warmed(board)?.2)?;
    let mounted = match corpus {
        bench::Corpus::Generated { mounted } => Some(mounted),
        bench::Corpus::Given => None,
    };
    let rows = against::rows(board, sample.as_str(), directory.as_str(), mounted);
    let compared = against::compare(&[base, new], &rows, options.runs, &host)?;
    report::emit(&Output::Against(&compared));
    Ok(against::passed(&compared))
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
