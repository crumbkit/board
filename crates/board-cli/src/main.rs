//! The `board` binary: it parses arguments, reads configuration once and
//! prints one report (docs/architecture.md, Commands). Every operation is in
//! `board-ops`.

mod alias;
mod complete;
mod completion;
mod config;
mod describe;
mod effects;
mod graft;
mod guessed;
mod init;
mod mistaken;
mod plugin;
mod render;
mod report;
mod run;
mod skills;
mod stdin;
mod timing;
mod ui;
mod values;

#[cfg(test)]
mod tests;

use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    complete::declare();
    let started = Instant::now();
    let context = config::Context::read();
    timing::watch(started, &context);
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
    // Appending the timing line may fail to mark a cache root the run
    // did not open.
    report::emit(&report::Output::default().warned());
    ExitCode::from(output.code)
}
