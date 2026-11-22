//! Which commit a run reads the mount holding its directory at.

use board_model::Refusal;
use board_ops::{CheckoutRefused, MountsAt};

use super::{Cli, Command};

impl Cli {
    /// Which commit this run reads the mount holding its directory at.
    /// `--checkout` refuses a command that opens no board or serves one
    /// past this run, and one that reads each mount at a commit's pin.
    pub(super) fn mounts_at(&self) -> Result<MountsAt, Box<Refusal>> {
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
}
