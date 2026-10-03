//! Board's pure types and decisions. Nothing here performs I/O.

mod code;
mod json;
mod location;
mod path;
mod pointer;
mod record;
mod stat;

pub use crate::code::Code;
pub use crate::json::{JsonError, parse_json};
pub use crate::location::{Board, Location, Object, ObjectId, Source};
pub use crate::path::{LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, Malformation, PathError};
pub use crate::pointer::{MAX_RECORD_BYTES, Mount, POINTER_TYPE, Pointer, PointerError, pointer};
pub use crate::record::{
    CONFIG_TYPE, Classified, Content, Kind, RecordType, RecordTypeError, Role, classify,
    needs_bytes,
};
pub use crate::stat::{Change, Stat};
