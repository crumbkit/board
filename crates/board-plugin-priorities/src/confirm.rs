//! `confirm`: record that the owner confirmed an entry, or every entry,
//! today.

use board_ops::{Host, PluginError};
use board_plugin::shape::Date;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::records::Horizon;
use crate::{held, not_found, refusal, today, write};

/// The entry to confirm, or every entry.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConfirmInput")]
pub(crate) struct Input {
    /// The id of the entry the owner confirmed.
    id: Option<String>,
    /// Confirm every entry in both horizons.
    #[serde(default)]
    all: bool,
}

/// The ids confirmed and the day recorded.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ConfirmOutput")]
pub(crate) struct Output {
    confirmed: Vec<String>,
    #[schemars(with = "Date")]
    day: String,
}

pub(crate) fn confirm(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let asked: Input = PluginError::read_input("priorities", "confirm", input)?;
    let (mut record, blob) = held(host)?;
    let day = today(host);
    let mut confirmed = Vec::new();
    match (asked.id, asked.all) {
        (Some(id), false) => {
            let (horizon, index) = record.find(&id).ok_or_else(|| not_found(&record, &id))?;
            record.entries_mut(horizon)[index].confirmed = Some(day.clone());
            confirmed.push(id);
        }
        (None, true) => {
            for horizon in Horizon::BOTH {
                for entry in record.entries_mut(horizon) {
                    entry.confirmed = Some(day.clone());
                    confirmed.push(entry.id.clone());
                }
            }
        }
        _ => {
            let mut refused = refusal(
                "priorities::confirm::unnamed",
                "confirm takes either --id or --all, and was given neither or both".to_owned(),
                &[],
            );
            refused.hint =
                Some("pass `--id <id>` for one entry or `--all` for every entry".to_owned());
            return Err(refused);
        }
    }
    write(host, &record, &blob)?;
    let output = Output { confirmed, day };
    Ok(serde_json::to_value(output).expect("the output is plain strings"))
}
