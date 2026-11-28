//! Which commit a run reads the mount holding its directory at, and the
//! commands `--board` does not apply to.

use board_model::{Code, Fields, Refusal};
use board_ops::{CheckoutRefused, MountsAt};

use super::{Cli, Command};

impl Cli {
    /// Which commit this run reads the mount holding its directory at.
    /// `--checkout` refuses a command that opens no board or serves one
    /// past this run, and one that reads each mount at a commit's pin;
    /// `--board` refuses a command it does not apply to.
    pub(super) fn mounts_at(&self) -> Result<MountsAt, Box<Refusal>> {
        self.through_applies()?;
        let command = match (self.checkout, &self.command) {
            (false, _) => return Ok(MountsAt::Pins),
            (true, Command::Init(_)) => "init",
            (true, Command::Answer(_)) => "answer",
            (true, Command::Serve { .. }) => "serve",
            (true, Command::Ui(_)) => "ui",
            (true, Command::Get { at: Some(_), .. }) => {
                return Err(CheckoutRefused::AtCommit("get --at").refusal());
            }
            (true, Command::Log(_)) => return Err(CheckoutRefused::AtCommit("log").refusal()),
            (true, _) => return Ok(MountsAt::Checkout),
        };
        Err(CheckoutRefused::Command(command).refusal())
    }

    /// The refusal of `--board` for a command it does not apply to: `init`
    /// makes a board in the directory Board starts in, and `answer` writes
    /// to the board each effect's asker names.
    fn through_applies(&self) -> Result<(), Box<Refusal>> {
        if self.through.is_none() {
            return Ok(());
        }
        let (command, why) = match &self.command {
            Command::Init(_) => (
                "init",
                "init makes a board in the directory Board starts in, which -C chooses",
            ),
            Command::Answer(_) => (
                "answer",
                "answer writes to the board each effect's asker names",
            ),
            _ => return Ok(()),
        };
        let message = format!("--board does not apply to {command}: {why}");
        let hint = format!("run {command} without --board");
        let refusal = Refusal::new(Code::CliUsage, message, &Fields::default()).with_hint(hint);
        Err(Box::new(refusal))
    }
}
