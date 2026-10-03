//! Board's pure types and decisions. Nothing here performs I/O.

mod app;
mod code;
mod path;

pub use crate::app::{BINARY_NAME, DISPLAY_NAME};
pub use crate::code::Code;
pub use crate::path::{LogicalPath, MAX_PATH_BYTES, MAX_SEGMENT_BYTES, Malformation, PathError};
