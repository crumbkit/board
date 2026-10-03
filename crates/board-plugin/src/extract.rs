//! Sending records to a plugin that extracts them (docs/plugins.md,
//! Extraction).

use std::collections::HashSet;
use std::path::Path;

use board_model::{ObjectId, canonical_json};
use serde_json::{Value, json};

use crate::call::{CallError, Context, exchange};
use crate::manifest::Manifest;
use crate::process::Limits;
use crate::protocol::{BadOutput, EXTRACT_RESULT_TYPE, EXTRACT_TYPE, RawExtractResult};

/// One record to extract.
#[derive(Clone, Debug)]
pub struct Record {
    pub blob: ObjectId,
    /// Its display path.
    pub path: String,
    pub record_type: String,
    pub record: Value,
}

/// What one extraction did: the blobs the plugin handled, and the failure
/// that stopped it, if one did. Blobs handled before a failure count.
#[derive(Debug, Default)]
pub struct Extraction {
    pub handled: Vec<ObjectId>,
    pub failure: Option<CallError>,
}

/// Sends `records` to `manifest`'s plugin in `directory`, in batches within
/// the context's limits, one run each.
pub fn extract(
    manifest: &Manifest,
    directory: &Path,
    records: Vec<Record>,
    context: &Context<'_>,
) -> Extraction {
    let mut extraction = Extraction::default();
    let depth = match context.environment.depth() {
        Ok(depth) => depth,
        Err(failure) => {
            extraction.failure = Some(failure);
            return extraction;
        }
    };
    for batch in batches(records, context.limits) {
        let sent: HashSet<ObjectId> = batch.iter().map(|(record, _)| record.blob).collect();
        let request = canonical_json(&json!({
            "$type": EXTRACT_TYPE,
            "records": batch.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "board": {
                "root": context.root.to_string_lossy(),
                "database": context.database.to_string_lossy(),
            },
        }));
        let result = exchange::<RawExtractResult>(
            manifest,
            directory,
            request,
            context,
            depth,
            EXTRACT_RESULT_TYPE,
        )
        .and_then(|result| handled(manifest, &sent, result.blobs));
        match result {
            Ok(blobs) => extraction.handled.extend(blobs),
            Err(failure) => {
                extraction.failure = Some(failure);
                break;
            }
        }
    }
    extraction
}

/// `records` in order, each with the JSON it is sent as, cut into batches.
fn batches(records: Vec<Record>, limits: Limits) -> Vec<Vec<(Record, Value)>> {
    let mut batches: Vec<Vec<(Record, Value)>> = Vec::new();
    let mut bytes = 0;
    for record in records {
        let value = json!({
            "blob": record.blob.to_string(),
            "path": record.path,
            "type": record.record_type,
            "record": record.record,
        });
        let size = value.to_string().len();
        match batches.last_mut() {
            Some(batch)
                if batch.len() < limits.batch_records && bytes + size <= limits.batch_bytes =>
            {
                bytes += size;
                batch.push((record, value));
            }
            _ => {
                bytes = size;
                batches.push(vec![(record, value)]);
            }
        }
    }
    batches
}

/// The blobs a reply names, each of which must have been sent.
fn handled(
    manifest: &Manifest,
    sent: &HashSet<ObjectId>,
    named: Vec<String>,
) -> Result<Vec<ObjectId>, CallError> {
    named
        .into_iter()
        .map(|text| {
            ObjectId::from_hex(&text)
                .filter(|blob| sent.contains(blob))
                .ok_or_else(|| CallError::BadOutput {
                    plugin: manifest.name.clone(),
                    reason: BadOutput::Unsent { blob: text },
                    stderr: Vec::new(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests;
