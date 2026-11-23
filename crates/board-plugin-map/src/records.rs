//! The record type the plugin ships. Its schema is generated from this
//! struct; the file under `types/` is that schema, committed.

use board_plugin::shape::{Prose, Record, Slug, Tag};

use crate::check::Kind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `map/layout/v1` at `/maps/<name>`: one map's sheet, regions, items,
/// lines and notes, with each item's stored position and label side.
///
/// Positions and sizes are in layout units, one unit to a CSS pixel when
/// the page draws the sheet at its own size, with x to the right and y
/// down from the sheet's top left corner. An item with no `at` has no
/// stored position yet, so the map does not draw it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/maps/"))]
pub struct LayoutRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// The map's title, such as `Services and their links`.
    #[schemars(with = "Prose", length(min = 1))]
    pub title: String,
    /// The sheet every label must stay on.
    pub sheet: Sheet,
    /// Titled rectangles that group items, such as one per system or one
    /// lane per stream of work; left out when the map has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub regions: Vec<Region>,
    /// What the map draws a mark and a label for.
    #[schemars(length(min = 1))]
    pub items: Vec<Item>,
    /// Lines between items; left out when the map has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub lines: Vec<Line>,
    /// Short texts placed beside an item or along a line; left out when the
    /// map has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub notes: Vec<Note>,
    /// The shades that an item's `bin` picks from, with what each stands
    /// for; left out when no item is shaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<Ramp>,
    /// How the page draws each kind of line, with its entry in the key; a
    /// kind with no entry is drawn solid, with an arrow, labelled by its
    /// name. Left out when the map styles none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub line_kinds: Vec<LineKind>,
    /// Tables shown under the map, one after another; left out when the
    /// map has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub tables: Vec<Table>,
    /// Findings the map's maker accepts, each with why, such as two lines
    /// that must leave an item in one direction. `map.check` counts them
    /// apart from its findings. Left out when the map accepts none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(min = 1))]
    pub accepts: Vec<Accept>,
}

impl Record for LayoutRecord {
    const TYPE: &'static str = "map/layout/v1";
}

/// The drawing area, from (0, 0) to (width, height).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    /// The sheet's width.
    #[schemars(range(min = 1))]
    pub width: f64,
    /// The sheet's height.
    #[schemars(range(min = 1))]
    pub height: f64,
}

/// A titled rectangle that groups items. Its title sits in its top left
/// corner, and a label of an item in it must stay inside it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Region {
    /// The region's name, unique among the map's regions, which an item's
    /// `region` names.
    #[schemars(with = "Slug")]
    pub id: String,
    /// The title drawn in the region's top left corner.
    #[schemars(with = "Prose", length(min = 1))]
    pub title: String,
    /// The left edge.
    pub x: f64,
    /// The top edge.
    pub y: f64,
    /// The width.
    #[schemars(range(min = 1))]
    pub width: f64,
    /// The height.
    #[schemars(range(min = 1))]
    pub height: f64,
}

/// One thing the map shows, drawn as a round mark with a label of one or
/// more lines beside it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// The item's name, unique among the map's items, which lines and notes
    /// name.
    #[schemars(with = "Slug")]
    pub id: String,
    /// The region the item belongs to; left out when it belongs to none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub region: Option<String>,
    /// The label's lines, first the name and then any detail, such as
    /// `["Board", "records in git"]`.
    #[schemars(with = "Vec<Prose>", length(min = 1), inner(length(min = 1)))]
    pub label: Vec<String>,
    /// The mark's radius; 8 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub radius: Option<f64>,
    /// The mark's stored centre; left out until it is placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    /// The side of the mark the label sits on; right when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    /// Which of the ramp's bins shades the mark, counted from 0 for the
    /// first; the mark is unshaded when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(max = 4))]
    pub bin: Option<u8>,
}

/// The shades of a map, from low to high, and what each stands for.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    /// What the shade measures, such as `Open tasks`.
    #[schemars(with = "Prose", length(min = 1))]
    pub label: String,
    /// What each bin stands for, lowest first, such as `none`, `1`, `2+`.
    #[schemars(with = "Vec<Prose>", length(min = 2, max = 5), inner(length(min = 1)))]
    pub bins: Vec<String>,
}

/// How lines of one kind are drawn.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineKind {
    /// The kind, as a line's `kind` names it.
    #[schemars(with = "Slug")]
    pub kind: String,
    /// What the key says the line means, such as `builds after`.
    #[schemars(with = "Prose", length(min = 1))]
    pub label: String,
    /// How the stroke is drawn; solid when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Stroke>,
    /// Whether the line ends in an arrow; true when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow: Option<bool>,
}

// How a line's stroke is drawn. No doc comment, which would describe the
// inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Stroke {
    Solid,
    Dashed,
    Dotted,
}

/// A table shown under the map.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// The table's name, unique among the map's tables.
    #[schemars(with = "Slug")]
    pub id: String,
    /// The heading above the table.
    #[schemars(with = "Prose", length(min = 1))]
    pub title: String,
    /// A sentence below the table; left out when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    pub caption: Option<String>,
    /// The columns, left to right.
    #[schemars(length(min = 1))]
    pub columns: Vec<Column>,
    /// The rows, top to bottom; each has one cell for each column.
    pub rows: Vec<Row>,
}

/// One column of a table.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// The column's heading.
    #[schemars(with = "Prose", length(min = 1))]
    pub label: String,
    /// Whether the column's cells are numbers, drawn right aligned; false
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric: Option<bool>,
}

/// One row of a table.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// The item the row describes, which the page lights with the row;
    /// left out when the row describes no item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Slug>")]
    pub item: Option<String>,
    /// The cells, one for each column.
    #[schemars(with = "Vec<Prose>")]
    pub cells: Vec<String>,
}

/// An item's stored centre and who set it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct At {
    /// The centre's x.
    pub x: f64,
    /// The centre's y.
    pub y: f64,
    /// engine when placement chose the position, which a later run with
    /// relayout may move, and pinned when a person or agent set it, which
    /// placement never moves.
    pub set: Set,
}

// Who set a stored position. No doc comment, which would describe the
// inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Set {
    Engine,
    Pinned,
}

// The side of its mark a label sits on. No doc comment, which would
// describe the inlined enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Side {
    #[default]
    Right,
    Left,
    Below,
    Above,
}

/// A line from one item to another, drawn between the edges of their
/// marks.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Line {
    /// The item the line starts at.
    #[schemars(with = "Slug")]
    pub from: String,
    /// The item the line ends at.
    #[schemars(with = "Slug")]
    pub to: String,
    /// What the line means, such as `library` or `after`, which the page
    /// gives a style of its own.
    #[schemars(with = "Slug")]
    pub kind: String,
}

/// A short text that belongs to an item or to a line, placed from its
/// anchor's position whenever the map is drawn, so it moves with what it
/// describes.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Note {
    /// The note's text.
    #[schemars(with = "Prose", length(min = 1))]
    pub text: String,
    /// What the note belongs to.
    pub anchor: Anchor,
}

/// What a note belongs to: an item, with the note's start offset from the
/// item's centre, or a line, with a point along it and an offset across it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Item(
        /// The item, and the note's start offset from its centre.
        ItemAnchor,
    ),
    Line(
        /// The line, and the note's point along it and offset across it.
        LineAnchor,
    ),
}

/// A note's start, offset from an item's centre.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemAnchor {
    /// The item.
    #[schemars(with = "Slug")]
    pub id: String,
    /// How far right of the centre the text starts.
    pub dx: f64,
    /// How far below the centre the text's baseline sits.
    pub dy: f64,
}

/// A note's place on a line: a fraction of the way from `from` to `to`,
/// then `offset` units across the line, to the right of the line where
/// `offset` is positive.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineAnchor {
    /// The line's first item.
    #[schemars(with = "Slug")]
    pub from: String,
    /// The line's second item.
    #[schemars(with = "Slug")]
    pub to: String,
    /// How far along the line, from 0 at `from` to 1 at `to`.
    #[schemars(range(min = 0, max = 1))]
    pub along: f64,
    /// How far across the line; 9 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

/// A finding the map's maker accepts: its kind and message as `map.check`
/// gives them, and why the map keeps it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Accept {
    /// The finding's kind.
    pub kind: Kind,
    /// The finding's message, word for word.
    #[schemars(with = "Prose", length(min = 1))]
    pub message: String,
    /// Why the map keeps what the finding names.
    #[schemars(with = "Prose", length(min = 1))]
    pub reason: String,
}
