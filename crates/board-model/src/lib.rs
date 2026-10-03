//! Board's pure types and decisions. Nothing here performs I/O.

mod code;
mod location;
mod path;

pub use crate::code::Code;
pub use crate::location::{Board, Location, Object, ObjectId, Source};
pub use crate::path::{LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, Malformation, PathError};
