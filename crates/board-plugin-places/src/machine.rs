//! The machine label, the second segment of every join's path. It is read
//! from the sources the personal board's runbook script reads, in the same
//! order, and made into a slug the same way, so both print the same label.

use std::iter::once_with;
use std::path::Path;
use std::time::Duration;

use board_ops::{Host, PluginError, Run};

use crate::board::refusal;

/// How long `scutil` or `hostname` may take to answer.
const SECONDS: u64 = 10;

/// The variable that names the machine, the first source.
pub(crate) const VARIABLE: &str = "BOARD_MACHINE";

/// The file whose first line names the machine, under `HOME`.
const FILE: &str = ".config/board/machine";

/// The label the first source that names anything gives, or none when no
/// source names anything or that name makes no valid label. Each source is
/// trimmed, and a blank one names nothing, so the next is read. The name is
/// lower-cased, each run of other characters than `a` to `z` and `0` to
/// `9` becomes `-`, and a `-` at either end is dropped. A label matches
/// `^[a-z0-9][a-z0-9-]*$`.
pub(crate) fn label(sources: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let name = (sources.into_iter())
        .flatten()
        .map(|text| text.trim().to_owned())
        .find(|text| !text.is_empty())?;
    let mut slug = String::new();
    for character in name.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    (!slug.is_empty()).then(|| slug.to_owned())
}

/// This machine's label: `BOARD_MACHINE`, else the first line of
/// `$HOME/.config/board/machine`, else `scutil --get LocalHostName`, else
/// `hostname -s`. A later source is read only when every earlier one names
/// nothing. With no label, it is `places::machine::unnamed`.
pub(crate) fn read(host: &dyn Host) -> Result<String, PluginError> {
    let environment = &host.settings().environment.0;
    let variable = |name: &str| {
        (environment.iter())
            .find(|(each, _)| each == name)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    // Each source is read only when `label` asks for it.
    let sources = once_with(|| variable(VARIABLE))
        .chain(once_with(|| {
            variable("HOME").and_then(|home| first_line(&Path::new(&home).join(FILE)))
        }))
        .chain(once_with(|| {
            printed(host, "scutil", &["--get", "LocalHostName"], true)
        }))
        .chain(once_with(|| printed(host, "hostname", &["-s"], false)));
    label(sources).ok_or_else(|| {
        refusal(
            "places::machine::unnamed",
            format!(
                "no source names this machine with a label of a to z, 0 to 9 and -: \
                 {VARIABLE} is unset or blank, ~/{FILE} holds no name, and scutil and \
                 hostname gave none"
            ),
            &[],
        )
    })
}

/// The first line of the file at `path`, or none when it cannot be read.
fn first_line(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().next().map(str::to_owned)
}

/// What `program` printed on stdout, or none when it did not run, or when
/// `succeeded` asks for a zero exit and it exited otherwise.
fn printed(host: &dyn Host, program: &str, arguments: &[&str], succeeded: bool) -> Option<String> {
    let arguments: Vec<String> = arguments.iter().map(|each| (*each).to_owned()).collect();
    let exited = host
        .run(&Run {
            program,
            arguments: &arguments,
            directory: None,
            input: b"",
            deadline: Duration::from_secs(SECONDS),
            added: &[],
        })
        .ok()?;
    if succeeded && exited.code != Some(0) {
        return None;
    }
    Some(String::from_utf8_lossy(&exited.stdout).into_owned())
}
