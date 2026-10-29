//! The effects commands: `decide` and `answer`.

use std::io::Read;
use std::path::{Path, PathBuf};

use board_model::{APPROVAL_HEADER, Answer, Code, Refusal};
use board_ops::{AnswerReport, Board, EffectError};

use crate::config::{ALLOWED_SIGNERS, Context};
use crate::known;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Given, Prefix, plain, read};
use crate::timing::Ran;

/// `board decide`: whether the plugin may run `op` at `path`, read from
/// `prefix`.
pub(crate) fn decide(
    board: &Board,
    printer: Printer,
    prefix: Option<&Prefix>,
    op: &str,
    path: &Given,
) -> Output {
    let decided = read(prefix, path, "decide")
        .and_then(|path| board.decide_op(op, &path).map_err(|error| refusal(&error)));
    match decided {
        Ok(report) => render::decide(printer, &report),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

/// The arguments of `board answer`.
#[derive(Debug, clap::Args)]
pub(crate) struct Answering {
    /// The pending effect's id, 64 hex digits.
    id: String,
    /// approve lets the op act, after the owner signs it with Touch ID; refuse refuses it.
    answer: Answer,
    /// Why, kept in the seen record.
    #[arg(long)]
    reason: Option<String>,
    /// A signature made elsewhere: this file, or - for stdin.
    #[arg(long, value_name = "FILE")]
    signature: Option<PathBuf>,
}

/// `board answer`, which needs no board but the personal one.
pub(crate) fn answer(
    context: &Context,
    printer: Printer,
    answering: Answering,
    stdin: &mut dyn Read,
) -> Output {
    match answered(context, answering, stdin) {
        Ok(report) => render::answer(printer, &report),
        Err(refusal) => render::refused(printer, *refusal),
    }
}

fn answered(
    context: &Context,
    answering: Answering,
    stdin: &mut dyn Read,
) -> Result<AnswerReport, Box<Refusal>> {
    let Answering {
        id,
        answer,
        reason,
        signature,
    } = answering;
    let given = signature.is_some();
    let signature = (signature.as_deref())
        .map(|file| supplied(answer, file, stdin))
        .transpose()?;
    let asker = known::asker(context, true)?;
    board_ops::answer(&asker, &id, answer, reason, signature).map_err(|error| {
        let refused = refusal(&error);
        match error.code() {
            Code::SettingsUnsigned if given => Box::new(refused.with_hint(unverified())),
            _ => refused,
        }
    })
}

/// The most bytes `--signature` reads. An armored SSH signature is under
/// 1 KiB, and the signature reaches the verifier as one argument.
pub(crate) const MAX_SIGNATURE_BYTES: usize = 8192;

/// The hint when a signature given with `--signature` does not verify.
fn unverified() -> String {
    format!(
        "the signature given with --signature did not verify: check that it is an SSH signature in the namespace {APPROVAL_HEADER} of this effect's statement, by a key ~/{ALLOWED_SIGNERS} lists as `owner`; or answer without --signature to sign with Touch ID"
    )
}

/// The signature `--signature` names, read from `file`, or stdin for `-`.
/// A refusal takes none.
fn supplied(answer: Answer, file: &Path, stdin: &mut dyn Read) -> Result<String, Box<Refusal>> {
    let usage =
        |message: String, hint: &str| Box::new(plain(Code::CliUsage, &message).with_hint(hint));
    if answer == Answer::Refuse {
        return Err(usage(
            "--signature was given with refuse, and a refusal is unsigned".to_owned(),
            "drop --signature to refuse, or answer approve",
        ));
    }
    let mut bytes = Vec::new();
    let limit = MAX_SIGNATURE_BYTES as u64 + 1;
    let read = if file.as_os_str() == "-" {
        stdin.take(limit).read_to_end(&mut bytes)
    } else {
        std::fs::File::open(file).and_then(|opened| opened.take(limit).read_to_end(&mut bytes))
    };
    let shown = file.display();
    if let Err(error) = read {
        return Err(usage(
            format!("the signature in {shown} cannot be read: {error}"),
            "give --signature the file that holds the armored signature, or - and the signature on stdin",
        ));
    }
    if bytes.len() > MAX_SIGNATURE_BYTES {
        return Err(usage(
            format!("the signature in {shown} is over {MAX_SIGNATURE_BYTES} bytes"),
            "give --signature the armored SSH signature alone, which is under 1 KiB",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        usage(
            format!("the signature in {shown} is not UTF-8"),
            "give --signature the armored SSH signature, which is ASCII text",
        )
    })
}

/// Refuses, under `BOARD_PLUGIN`, the command `ran` names: a write the put
/// check does not cover, which only the owner runs. `None` for the owner.
pub(crate) fn from_plugin(context: &Context, ran: &Ran) -> Option<Box<Refusal>> {
    let plugin = match known::plugin(context) {
        Ok(plugin) => plugin?,
        Err(refusal) => return Some(refusal),
    };
    let what = format!("run board {}", ran.command.as_deref().unwrap_or_default());
    let error = EffectError::FromPlugin {
        plugin,
        path: None,
        what,
    };
    Some(refusal(&error))
}

fn refusal(error: &EffectError) -> Box<Refusal> {
    Box::new(error.refusal())
}
