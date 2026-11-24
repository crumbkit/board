//! The command a run names in its timing line, noted for the line a signal
//! writes (docs/architecture/performance/timing.md).

use std::ffi::OsString;

use board_plugin::Target;
use clap::CommandFactory;

use super::{Cli, Command};

/// The subcommands `words` run, which the timing line names, noted for
/// the line a signal writes with the capability the command line calls
/// when it names one. `serve` and `ui` stop on signals themselves, so a
/// signal writes no line for them. `timing` runs detached under nohup, and
/// the signal thread would end it on the SIGHUP nohup has it ignore.
pub(super) fn watched(
    words: &[OsString],
    command: &Command,
    grafted: Option<&(Target, serde_json::Map<String, serde_json::Value>)>,
) -> String {
    let named = crumbkit::command_path(&Cli::command(), words).join(" ");
    if !matches!(
        command,
        Command::Serve { .. } | Command::Ui(_) | Command::Timing(_)
    ) {
        crate::timing::naming(&named, called(command, grafted));
    }
    named
}

/// The capability a command line calls, when it names one.
fn called(
    command: &Command,
    grafted: Option<&(Target, serde_json::Map<String, serde_json::Value>)>,
) -> Option<String> {
    let ((Some((target, _)), _)
    | (
        None,
        Command::Call {
            target: Some(target),
            ..
        },
    )) = (grafted, command)
    else {
        return None;
    };
    match target {
        Target::Named { plugin, capability } => {
            Some(format!("{}.{}", plugin.as_str(), capability.as_str()))
        }
        Target::Capability(_) => None,
    }
}
