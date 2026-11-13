//! board-top: a terminal view of the changes in play on a board.
//!
//! The data layer (`source`) runs `board` and `git`; `parse` turns their text
//! into records; `model` builds rows, groups and counts; `render` draws them.

pub mod app;
pub mod model;
pub mod parse;
pub mod render;
pub mod source;
pub mod time;
