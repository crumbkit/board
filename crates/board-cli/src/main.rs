//! The `board` binary: it parses arguments, reads configuration once and
//! prints one report (docs/architecture.md, Commands). Every operation is in
//! `board-ops`.

mod config;
mod render;
mod report;
mod run;

#[cfg(test)]
mod tests;

use std::process::ExitCode;

fn main() -> ExitCode {
    let context = config::Context::read();
    let output = run::run(std::env::args_os(), &context);
    report::emit(&output);
    ExitCode::from(output.code)
}
