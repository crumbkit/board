//! The `board` binary: it parses arguments, reads configuration once and
//! prints one report (docs/architecture.md, Commands). Every operation is in
//! `board-ops`.

mod complete;
mod config;
mod effects;
mod graft;
mod known;
mod plugin;
mod render;
mod report;
mod run;
mod stdin;
mod timing;
mod ui;

#[cfg(test)]
mod tests;

use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let started = Instant::now();
    let context = config::Context::read();
    let mut ran = timing::Ran::default();
    let output = run::run(
        std::env::args_os(),
        &context,
        &mut std::io::stdin().lock(),
        &mut ran,
    );
    let emitting = Instant::now();
    report::emit(&output);
    ran.trace.add("emit", emitting);
    timing::append(started, &ran, &output, &context);
    ExitCode::from(output.code)
}
