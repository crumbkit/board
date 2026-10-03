//! Pointers: records of type `board/mount/v1` that resolution continues
//! through (docs/architecture.md, Pointers: links and mounts).

use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::code::Code;
use crate::path::LogicalPath;

/// The `$type` of a pointer.
pub const POINTER_TYPE: &str = "board/mount/v1";

/// The largest record Board reads, in bytes.
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

/// Where a pointer leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// A path in the board that holds the link.
    Link(LogicalPath),
}

#[derive(Deserialize)]
struct RawTag {
    #[serde(rename = "$type")]
    tag: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLink {
    #[serde(rename = "$type")]
    _tag: IgnoredAny,
    path: LogicalPath,
}

/// The pointer `bytes` hold, if they are a record whose `$type` is
/// [`POINTER_TYPE`]. Anything else, including bytes that are not a JSON
/// object, is not a pointer; deciding what it is instead is not this
/// function's job.
pub fn pointer(bytes: &[u8]) -> Result<Option<Pointer>, PointerError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Ok(None);
    }
    match serde_json::from_slice::<RawTag>(bytes) {
        Ok(RawTag { tag: Some(tag) }) if tag == POINTER_TYPE => {}
        _ => return Ok(None),
    }
    let link: RawLink =
        serde_json::from_slice(bytes).map_err(|source| PointerError::Invalid { source })?;
    Ok(Some(Pointer::Link(link.path)))
}

/// A record that claims to be a pointer but is not a valid one.
#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("pointer is invalid: {source}")]
    Invalid {
        #[source]
        source: serde_json::Error,
    },
}

impl PointerError {
    pub fn code(&self) -> Code {
        match self {
            Self::Invalid { .. } => Code::MountInvalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(path: &str) -> Pointer {
        Pointer::Link(path.parse().unwrap())
    }

    #[test]
    fn a_link_is_a_pointer_record_with_one_path() {
        let bytes = br#"{"$type": "board/mount/v1", "path": "/work/x"}"#;
        assert_eq!(pointer(bytes).unwrap(), Some(link("/work/x")));
    }

    #[test]
    fn a_record_of_another_type_is_not_a_pointer() {
        for bytes in [
            &br#"{"$type": "lr/task/v1", "path": "/work/x"}"#[..],
            br#"{"path": "/work/x"}"#,
            br#"{"$type": 1}"#,
            b"not json",
            b"[]",
        ] {
            assert_eq!(pointer(bytes).unwrap(), None);
        }
    }

    #[test]
    fn a_pointer_with_an_unknown_field_is_invalid() {
        let admitted = br#"{"$type": "board/mount/v1", "path": "/x"}"#;
        assert!(pointer(admitted).is_ok());
        let refused = br#"{"$type": "board/mount/v1", "path": "/x", "note": 1}"#;
        assert_eq!(pointer(refused).unwrap_err().code(), Code::MountInvalid);
    }

    #[test]
    fn a_pointer_whose_path_is_malformed_or_missing_is_invalid() {
        for refused in [
            &br#"{"$type": "board/mount/v1", "path": "x"}"#[..],
            br#"{"$type": "board/mount/v1"}"#,
            br#"{"$type": "board/mount/v1", "path": "/x", "path": "/y"}"#,
        ] {
            assert_eq!(pointer(refused).unwrap_err().code(), Code::MountInvalid);
        }
    }

    #[test]
    fn a_record_over_the_size_limit_is_never_read_as_a_pointer() {
        let small = format!(
            r#"{{"$type": "board/mount/v1", "path": "/x"}}{}"#,
            " ".repeat(64)
        );
        assert!(pointer(small.as_bytes()).unwrap().is_some());
        let large = format!(
            r#"{{"$type": "board/mount/v1", "path": "/x"}}{}"#,
            " ".repeat(MAX_RECORD_BYTES)
        );
        assert_eq!(pointer(large.as_bytes()).unwrap(), None);
    }
}
