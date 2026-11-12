//! Where the changes plugin stores a version of a flow. The rules plugin's
//! rename stores flows again under the new names, so both compute the path
//! here.

use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::json::canonical_json;

/// How many hex digits of the SHA-256 a version keeps.
const VERSION_DIGITS: usize = 16;

/// Where the flow named `name` whose JSON is `value` is stored:
/// `/flows/<name>/<version>`, the version being the first 16 hex digits of
/// the SHA-256 of the flow's canonical JSON, so identical flows share one
/// path.
pub fn flow_version(name: &str, value: &Value) -> String {
    let digest = Sha256::digest(canonical_json(value).as_str().as_bytes());
    let hex: String =
        (digest.iter().take(VERSION_DIGITS / 2)).fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    format!("/flows/{name}/{hex}")
}
