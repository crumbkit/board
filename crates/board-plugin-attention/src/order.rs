//! Reading an order and its context file into the `Order` whose statement
//! the owner signs. Every value the statement copies is checked here, so
//! `verify` and `sign` never build a statement that reads as another
//! record's.

use board_model::{Expiry, Fields, LogicalPath, ORDER_TYPE, Object, Order, Refusal};
use board_ops::{Host, PluginError};
use board_repos::Date;
use serde_json::{Value, json};

use crate::records::OrderRecord;
use crate::{refusal, refused};

/// The most bytes of context an order may carry. The owner reads all of it
/// before signing, so it stays short enough to read.
pub(crate) const CONTEXT_LIMIT: usize = 64 * 1024;

/// The start of the line that begins the context in an order's statement.
/// An order whose text holds a line that starts with it could be read as
/// a statement whose context starts earlier.
const CONTEXT_HEADER: &str = "Context (non-binding, from ";

/// An order read from the board, ready to verify or sign.
pub(crate) struct Held {
    /// The record as stored.
    pub(crate) record: Value,
    /// The blob that holds it, which a put expects.
    pub(crate) blob: String,
    pub(crate) fields: OrderRecord,
    pub(crate) expires: Date,
    pub(crate) order: Order,
}

/// `attention::order::invalid` for the order at `path`.
fn invalid(path: &str, why: &str) -> PluginError {
    refusal(
        "attention::order::invalid",
        format!("{path} is not a readable order: {why}"),
        [("path", json!(path))],
    )
}

/// The file at `path` and the blob that holds it, as `board get` reads it.
/// A directory has no blob.
fn get(host: &dyn Host, path: &str) -> Result<(Vec<u8>, Option<String>), PluginError> {
    let logical: LogicalPath = path.parse().map_err(|error: board_model::PathError| {
        let refusal = Refusal::new(error.code(), &error, &Fields::default());
        refused("get", &refusal)
    })?;
    let answers =
        (host.get_each(&[logical])).map_err(|error| refused("get", &error.refusal("get")))?;
    let Some(answer) = answers.into_iter().next() else {
        let why = format!("board get gave no answer for {path}");
        return Err(refusal(
            "attention::board::refused",
            why,
            [("path", json!(path))],
        ));
    };
    let report = answer.map_err(|error| refused("get", &error.refusal("get")))?;
    let blob = match &report.location.object {
        Object::Blob(id) => Some(id.to_string()),
        Object::Directory => None,
    };
    Ok((report.bytes, blob))
}

/// The order at `path` and its context, each value checked before the
/// statement copies it.
pub(crate) fn read(host: &dyn Host, path: &str) -> Result<Held, PluginError> {
    let (bytes, blob) = get(host, path)?;
    let not_an_order = || {
        refusal(
            "attention::order::not_an_order",
            format!(
                "{path} is not a {ORDER_TYPE} record; give the path of an order, such as /orders/<name>"
            ),
            [("path", json!(path))],
        )
    };
    let blob = blob.ok_or_else(not_an_order)?;
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|error| invalid(path, &error.to_string()))?;
    if record.get("$type").and_then(Value::as_str) != Some(ORDER_TYPE) {
        return Err(not_an_order());
    }
    let fields = serde_json::from_value::<OrderRecord>(record.clone())
        .map_err(|error| invalid(path, &error.to_string()))?;
    let expires = checked(path, &fields)?;
    let context = context(host, &fields.context)?;
    let order = Order {
        order: fields.order.clone(),
        context_path: fields.context.clone(),
        expires: expiry(expires),
        pinned: fields.pinned.into(),
        context_text: context,
    };
    Ok(Held {
        record,
        blob,
        fields,
        expires,
        order,
    })
}

/// The order's expiry, once its text, its context path and its expiry are
/// each shown to be what the statement can hold.
pub(crate) fn checked(path: &str, fields: &OrderRecord) -> Result<Date, PluginError> {
    if fields.order.is_empty() {
        return Err(invalid(
            path,
            "its order is empty; write the instruction the owner signs",
        ));
    }
    if fields
        .order
        .split('\n')
        .any(|line| line.starts_with(CONTEXT_HEADER))
    {
        let why = format!(
            "its order holds a line that starts {CONTEXT_HEADER:?}, which the statement uses to \
             start the context; reword that line"
        );
        return Err(invalid(path, &why));
    }
    if fields.context.contains(['\n', '\r']) {
        let why = "its context path holds a line break; name the context file by its path alone";
        return Err(invalid(path, why));
    }
    Date::parse(&fields.expires).ok_or_else(|| {
        let why = format!(
            "its expires, {:?}, is not a calendar date; write the last day the order binds as \
             YYYY-MM-DD",
            fields.expires
        );
        invalid(path, &why)
    })
}

/// The `Expiry` of a date that `Date::parse` read from four digits.
fn expiry(date: Date) -> Expiry {
    Expiry {
        year: u16::try_from(date.year).expect("Date::parse reads a year of four digits"),
        month: date.month,
        day: date.day,
    }
}

/// The text of the context file at `path`, at most [`CONTEXT_LIMIT`] bytes
/// of UTF-8.
fn context(host: &dyn Host, path: &str) -> Result<String, PluginError> {
    let (bytes, _) = get(host, path)?;
    if bytes.len() > CONTEXT_LIMIT {
        return Err(refusal(
            "attention::order::context_too_large",
            format!(
                "{path} holds {} bytes, over the {CONTEXT_LIMIT} the owner can read before signing; \
                 shorten it",
                bytes.len()
            ),
            [
                ("path", json!(path)),
                ("bytes", json!(bytes.len())),
                ("limit", json!(CONTEXT_LIMIT)),
            ],
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        refusal(
            "attention::order::invalid",
            format!("{path} is not UTF-8 text"),
            [("path", json!(path))],
        )
    })
}
