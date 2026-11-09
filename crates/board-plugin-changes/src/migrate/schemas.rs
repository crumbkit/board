//! Checks a migration's rewritten records against the plugin's own schemas.

use std::collections::BTreeMap;

use board_ops::Plugin;
use board_validate::Schema;
use serde_json::Value;

/// The plugin's own schemas, compiled, by type.
pub(super) fn compiled() -> BTreeMap<String, Schema> {
    (crate::Changes.schemas().into_iter())
        .filter_map(|(kind, document)| {
            Some((kind.as_str().to_owned(), Schema::compile(document).ok()?))
        })
        .collect()
}

/// Whether every record among `lines` passes its type's schema, so a dry
/// run that lists a change promises an apply that writes it. A record of a
/// type the plugin does not ship is left to Board's own check.
pub(super) fn conforms(schemas: &BTreeMap<String, Schema>, lines: &[Value]) -> Result<(), String> {
    for line in lines {
        let Some(record) = line.get("record").or_else(|| line.get("change")) else {
            continue;
        };
        let kind = record
            .get("$type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(schema) = schemas.get(kind)
            && let Err(error) = schema.validate(record)
        {
            let at = line.get("path").and_then(Value::as_str).unwrap_or_default();
            return Err(format!(
                "its rewritten record at {at} {error}, so nothing of it is written; fix the old \
                 record and run migrate again"
            ));
        }
    }
    Ok(())
}
