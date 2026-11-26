//! What `board orders pinned` and `board orders signers` print as text: the
//! signing line first, then a row per order, or the signers file's lines.
//! The JSON they return holds the same and more.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

/// The text of `capability`'s `output`, or none for a capability whose
/// output prints as JSON. `home` abbreviates paths under it to `~/`.
pub(crate) fn text(capability: &str, output: &Value, home: Option<&Path>) -> Option<String> {
    let at_home = |line: &str| abbreviated(line, home);
    match capability {
        "pinned" => Some(pinned(output, &at_home)),
        "signers" => Some(signers(output, &at_home)),
        _ => None,
    }
}

/// `line` with each path under `home` written from `~/`. It replaces the
/// home directory followed by `/` wherever it appears, which is enough for
/// the paths Board names in these lines; a path that merely starts with the
/// same letters as `home` is left as it is.
fn abbreviated(line: &str, home: Option<&Path>) -> String {
    let Some(home) = home
        .and_then(Path::to_str)
        .map(|home| home.trim_end_matches('/'))
    else {
        return line.to_owned();
    };
    if home.is_empty() {
        return line.to_owned();
    }
    line.replace(&format!("{home}/"), "~/")
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn each<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// The signing line, a blank line, and a row per order: the orders left
/// out first, by their code, then each order in force with how the owner
/// approved it and when it expires.
fn pinned(output: &Value, at_home: &dyn Fn(&str) -> String) -> String {
    let (left_out, orders) = (each(output, "left_out"), each(output, "orders"));
    let mut text = format!("{}\n\n", at_home(field(output, "signing")));
    if left_out.is_empty() && orders.is_empty() {
        text.push_str("No order is pinned.\n");
        return text;
    }
    let status = if left_out.is_empty() { "ok" } else { "refused" };
    let path = (left_out.iter().chain(orders))
        .map(|row| field(row, "path").chars().count())
        .max()
        .unwrap_or_default();
    let approval = (orders.iter())
        .map(|row| field(row, "approval").chars().count())
        .max()
        .unwrap_or_default();
    let width = status.len();
    for row in left_out {
        let line = format!(
            "{:<width$} {:<path$}  {}",
            "refused",
            field(row, "path"),
            field(row, "code")
        );
        text.push_str(line.trim_end());
        text.push('\n');
    }
    for row in orders {
        let _ = writeln!(
            text,
            "{:<width$} {:<path$}  {:<approval$}  expires {}",
            "ok",
            field(row, "path"),
            field(row, "approval"),
            field(row, "expires")
        );
    }
    text
}

/// The signing line, a comment naming the file, the file's lines, and a
/// comment for each line that differs from Board's file or that the
/// attention file gave nothing.
fn signers(output: &Value, at_home: &dyn Fn(&str) -> String) -> String {
    let file = at_home(field(output, "file"));
    let lines = field(output, "text");
    let mut text = format!("{}\n", at_home(field(output, "signing")));
    if lines.is_empty() {
        let _ = writeln!(text, "# {file} (no lines)");
    } else {
        let _ = write!(text, "# {file}\n{lines}");
        if !lines.ends_with('\n') {
            text.push('\n');
        }
    }
    let number = |row: &Value, key: &str| row.get(key).and_then(Value::as_u64).unwrap_or_default();
    for row in each(output, "changes") {
        let what = match field(row, "change") {
            "gained" => "Board's line, with the orders namespace the attention file gives it",
            "added" => "added from the attention file, for orders alone",
            "merged" => "Board's lines for one key, printed as one",
            other => other,
        };
        let _ = writeln!(text, "# line {}: {what}", number(row, "line"));
    }
    for row in each(output, "duplicates") {
        let lines: Vec<String> = (each(row, "lines").iter())
            .filter_map(Value::as_u64)
            .map(|line| line.to_string())
            .collect();
        let merged = row.get("merged").and_then(Value::as_bool) == Some(true);
        let _ = writeln!(
            text,
            "# Board's file lists {} on lines {}{}",
            field(row, "key"),
            lines.join(", "),
            if merged { ", printed as one" } else { "" }
        );
    }
    for row in each(output, "left_out") {
        let why = match field(row, "reason") {
            "not_owner" => "its principals do not name owner",
            "not_orders" => "its namespaces leave out orders",
            "differs" => "Board's lines for its key name other principals or options",
            "pattern" => "it or Board's line for its key uses a pattern, which is not compared",
            "unreadable" => "it cannot be read",
            "duplicate" => "an earlier line already gave its key",
            "already_admitted" => "Board's lines for its key already admit owner in orders",
            other => other,
        };
        let _ = writeln!(
            text,
            "# the attention file's line {} gave nothing: {why}",
            number(row, "line")
        );
    }
    text
}
