//! Board's pure types and decisions. Nothing here performs I/O.

mod code;
mod location;
mod path;
mod pointer;

pub use crate::code::Code;
pub use crate::location::{Board, Location, Object, ObjectId, Source};
pub use crate::path::{LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, Malformation, PathError};
pub use crate::pointer::{MAX_RECORD_BYTES, Mount, POINTER_TYPE, Pointer, PointerError, pointer};
