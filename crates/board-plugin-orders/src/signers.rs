//! `signers`: Board's allowed signers file with the attention file's keys
//! merged in for orders, which the owner reads and writes to Board's file
//! themselves. It writes neither file.

use board_ops::{Host, PluginError, VerifyOrderError};
use board_plugin::{LeftOutReason, LineChange, Signers};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::refusal;

/// No input: `signers` reads the two files Board was given.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SignersInput")]
pub(crate) struct Input {}

/// How a printed line differs from Board's file.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Change {
    /// Board's line for a key the attention file lists, with the order
    /// namespace added.
    Gained,
    /// A key only the attention file lists, admitted for orders alone.
    Added,
    /// Board's lines for one key, printed as one.
    Merged,
}

/// A printed line, numbered from 1, that differs from Board's file.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Changed {
    pub(crate) line: usize,
    pub(crate) change: Change,
}

/// A key Board's file lists on more than one line, and those lines.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct Duplicate {
    /// The key, as `<type> <base64>`.
    pub(crate) key: String,
    pub(crate) lines: Vec<usize>,
    /// Whether they print as one line, which they do only when their
    /// principals and other options are the same.
    pub(crate) merged: bool,
}

/// Why a line of the attention file gave the printed file nothing.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    /// Its principals do not name `owner`.
    NotOwner,
    /// Its `namespaces=` leaves out orders.
    NotOrders,
    /// Board's lines for its key name other principals or other options.
    Differs,
}

/// A line of the attention file, numbered from 1, that gave the printed
/// file nothing.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct LeftOut {
    pub(crate) line: usize,
    /// The key, as `<type> <base64>`.
    pub(crate) key: String,
    pub(crate) reason: Reason,
}

/// The merged lines and what changed.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "SignersOutput", deny_unknown_fields)]
pub(crate) struct Output {
    /// Board's allowed signers file, which the owner writes `text` to.
    pub(crate) file: String,
    /// Every line of Board's file, with the attention file's keys merged in.
    pub(crate) text: String,
    pub(crate) changes: Vec<Changed>,
    pub(crate) duplicates: Vec<Duplicate>,
    pub(crate) left_out: Vec<LeftOut>,
}

pub(crate) fn signers(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {} = PluginError::read_input("orders", "signers", input)?;
    let merged = host.order_signers().map_err(|error| unread(&error))?;
    Ok(serde_json::to_value(output(merged)).expect("the output is plain strings and numbers"))
}

pub(crate) fn output(merged: Signers) -> Output {
    let change = |change| match change {
        LineChange::Gained => Change::Gained,
        LineChange::Added => Change::Added,
        LineChange::Merged => Change::Merged,
    };
    Output {
        file: merged.file.display().to_string(),
        text: merged.text,
        changes: (merged.changes.into_iter())
            .map(|changed| Changed {
                line: changed.line,
                change: change(changed.change),
            })
            .collect(),
        duplicates: (merged.duplicates.into_iter())
            .map(|duplicate| Duplicate {
                key: duplicate.key,
                lines: duplicate.lines,
                merged: duplicate.merged,
            })
            .collect(),
        left_out: (merged.left_out.into_iter())
            .map(|left| LeftOut {
                line: left.line,
                key: left.key,
                reason: match left.reason {
                    LeftOutReason::NotOwner => Reason::NotOwner,
                    LeftOutReason::NotOrders => Reason::NotOrders,
                    LeftOutReason::Differs => Reason::Differs,
                },
            })
            .collect(),
    }
}

/// The refusal of signers files that could not be read, by why.
pub(crate) fn unread(error: &VerifyOrderError) -> PluginError {
    match error {
        VerifyOrderError::NoneNamed | VerifyOrderError::NoFile { .. } => refusal(
            "orders::signers::missing",
            format!("{error}; Board's allowed signers file is not named"),
            [],
        ),
        VerifyOrderError::Untrusted { file, .. } => refusal(
            "orders::signers::untrusted",
            format!(
                "{error}; make the owner the file's owner and let no one else write it, as with \
                 `chmod 644 {}`, then read its lines and run this again",
                file.display()
            ),
            [("file", json!(file))],
        ),
        VerifyOrderError::Bad { .. }
        | VerifyOrderError::Failed { .. }
        | VerifyOrderError::NotOffered => refusal(
            "orders::signers::failed",
            format!("the allowed signers files were not read: {error}"),
            [],
        ),
    }
}
