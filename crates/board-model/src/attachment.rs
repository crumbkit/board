//! Attachments: records of type `board/attachment/v1` that name bytes held
//! outside the tree by their digest (docs/architecture/attachments.md).

use std::fmt;
use std::str::FromStr;

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// The `$type` of an attachment.
pub const ATTACHMENT_TYPE: &str = "board/attachment/v1";

/// The SHA-256 of a file's bytes, spelled `sha256:<64 hex digits>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest([u8; 32]);

impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The 64 hex digits alone, which name the object in a store.
    pub fn hex(&self) -> String {
        use std::fmt::Write as _;
        self.0.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "sha256:{}", self.hex())
    }
}

impl FromStr for Digest {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let malformed = || format!("{text:?} is not sha256: followed by 64 hex digits");
        let hex = text.strip_prefix("sha256:").ok_or_else(malformed)?;
        if hex.len() != 64 {
            return Err(malformed());
        }
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| malformed())?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| malformed())?;
        }
        Ok(Self(bytes))
    }
}

impl TryFrom<String> for Digest {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl From<Digest> for String {
    fn from(digest: Digest) -> Self {
        digest.to_string()
    }
}

/// A `board/attachment/v1` record: the bytes a put uploaded, by their
/// digest, size and media type, and where the store holds them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Attachment {
    #[serde(rename = "$type")]
    _tag: &'static str,
    pub digest: Digest,
    pub size: u64,
    pub media_type: String,
    pub location: String,
}

/// Deserializing accepts any `$type`; classification has already read it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    digest: Digest,
    size: u64,
    media_type: String,
    location: String,
}

impl Attachment {
    pub fn new(digest: Digest, size: u64, media_type: String, location: String) -> Self {
        Self {
            _tag: ATTACHMENT_TYPE,
            digest,
            size,
            media_type,
            location,
        }
    }

    /// The attachment a parsed record of type [`ATTACHMENT_TYPE`] holds.
    pub fn from_value(value: &Value) -> Result<Self, serde_json::Error> {
        let raw = Raw::deserialize(value)?;
        Ok(Self::new(
            raw.digest,
            raw.size,
            raw.media_type,
            raw.location,
        ))
    }
}

/// The media type a file's name says it holds, by its extension, or
/// `application/octet-stream`.
pub fn media_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map_or("", |(_, extension)| extension) {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" => "text/html",
        "mp4" => "video/mp4",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests;
