//! `pinned`: every pinned order in the board's own tree that verifies, for
//! an agent session to start with, and the code of each one left out.

use board_model::ORDER_TYPE;
use board_ops::{Host, Limit, PluginError, literal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{refused, verify};

/// The most bytes of orders and context `pinned` shows, so the pins a
/// session starts with stay a small part of its context.
pub(crate) const PINNED_LIMIT: usize = 32 * 1024;

/// No input: `pinned` reads the pinned orders in the board's own tree, not
/// orders in boards it mounts.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PinnedInput")]
pub(crate) struct Input {}

/// A pinned order `pinned` did not show, and the code that says why.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(crate) struct LeftOut {
    pub(crate) path: String,
    pub(crate) code: String,
    pub(crate) message: String,
}

/// The pinned orders that verify, by path, and those left out.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "PinnedOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) orders: Vec<verify::Output>,
    pub(crate) left_out: Vec<LeftOut>,
}

/// Every pinned order that verifies, by path, while their text fits; the
/// rest left out with their codes. The query finds the board's own records
/// that say they are pinned, since a mounted board's pinned orders are not
/// this board's. Only an order whose signature verifies is shown, so an
/// agent cannot pin an order by editing it.
pub(crate) fn pinned(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {} = PluginError::read_input("attention", "pinned", input)?;
    let sql = format!(
        "SELECT logical FROM records WHERE type = {} AND json_extract(body, '$.pinned') = 1 \
         AND logical IS NOT NULL AND source = 'worktree' ORDER BY logical",
        literal(ORDER_TYPE)
    );
    let report =
        (host.query(&sql, Limit::ALL)).map_err(|error| refused("query", &error.refusal()))?;
    let mut output = Output {
        orders: Vec::new(),
        left_out: Vec::new(),
    };
    let mut used = 0;
    for path in (report.rows.iter()).filter_map(|row| row.first()?.as_str()) {
        let order = match verify::verified(host, path) {
            Ok(order) => order,
            Err(refused) => {
                output.left_out.push(LeftOut {
                    path: path.to_owned(),
                    code: refused.code,
                    message: refused.message,
                });
                continue;
            }
        };
        used += order.order.len() + order.context_text.len();
        if used > PINNED_LIMIT {
            output.left_out.push(LeftOut {
                path: path.to_owned(),
                code: "attention::pinned::over_budget".to_owned(),
                message: format!(
                    "the pinned orders before it hold more than {PINNED_LIMIT} bytes; read it \
                     with board call attention.verify, or unpin an order"
                ),
            });
            continue;
        }
        output.orders.push(order);
    }
    Ok(serde_json::to_value(output).expect("the output is plain strings"))
}
