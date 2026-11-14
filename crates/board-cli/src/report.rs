//! The one module that writes to stdout and stderr.

use std::io::Write;

/// What one run prints, and its exit code: 0 done, 1 failed, 2 the command
/// line was refused.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Output {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) code: u8,
    /// The code of the error reported, if one was.
    pub(crate) error: Option<String>,
}

impl Output {
    /// This output, with a warning on stderr when the run wrote a board whose
    /// `min_board` names a build date that this build, having none, could
    /// not check.
    pub(crate) fn warned_by(mut self, board: &board_ops::Board) -> Self {
        if let Some(note) = board.undated_gate_passed() {
            self.stderr
                .extend(format!("warning: {note}\n").into_bytes());
        }
        self
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "this module is the binary's one owner of stdout and stderr"
)]
pub(crate) fn emit(output: &Output) {
    // A closed pipe is not an error worth reporting; there is nowhere left
    // to report it.
    let _ = std::io::stdout().lock().write_all(&output.stdout);
    let _ = std::io::stderr().lock().write_all(&output.stderr);
}
