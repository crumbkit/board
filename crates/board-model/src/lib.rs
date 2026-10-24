//! Board's pure types and decisions. Nothing here performs I/O.

mod app;
mod attachment;
mod code;
mod fields;
mod json;
mod location;
mod markdown;
mod path;
mod pointer;
mod record;
mod refusal;
mod stat;

pub use crate::app::{BINARY_NAME, DISPLAY_NAME};
pub use crate::attachment::{ATTACHMENT_TYPE, Attachment, Digest, media_type};
pub use crate::code::Code;
pub use crate::fields::{Fields, Found, Known, Lined};
pub use crate::json::{Canonical, JsonError, canonical_json, parse_json};
pub use crate::location::{Board, Location, Object, ObjectId, Source};
pub use crate::markdown::{links, reached};
pub use crate::path::{
    LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, MAX_SYMLINKS, Malformation, PathError,
};
pub use crate::pointer::{
    MAX_RECORD_BYTES, Mount, POINTER_TYPE, Pointer, PointerError, pointer, repinned,
};
pub use crate::record::{
    CONFIG_TYPE, Classified, Content, Kind, MANIFEST_TYPE, RecordType, RecordTypeError, Role,
    TYPES_DIRECTORY, classify, needs_bytes,
};
pub use crate::refusal::{Refusal, RefusalCode};
pub use crate::stat::{Change, Stat};
