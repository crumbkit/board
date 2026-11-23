//! The record type the plugin ships. Its schema is generated from these
//! structs; the file under `types/` is that schema, committed.

use board_plugin::shape::{Date, Record, Slug, Summary, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// The most entries one horizon holds.
pub const MOST_ENTRIES: usize = 7;

/// The most records one entry covers.
pub const MOST_COVERS: usize = 20;

/// The record types an entry may cover.
pub const COVERED_TYPES: [&str; 3] = ["changes/change/v2", "tasks/task/v2", "tasks/task/v3"];

/// `priorities/list/v1`: the owner's priorities, ranked, in two horizons.
/// An entry's rank is its place in its list, highest first. Sharing a
/// record that an entry covers never copies this record, which
/// `x-board-goes-with-share` says.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-goes-with-share" = false))]
pub struct PrioritiesRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub tag: Tag<Self>,
    /// This week's priorities, highest first.
    #[schemars(length(max = MOST_ENTRIES))]
    pub short: Vec<Entry>,
    /// Priorities beyond this week, highest first.
    #[schemars(length(max = MOST_ENTRIES))]
    pub long: Vec<Entry>,
}

impl Record for PrioritiesRecord {
    const TYPE: &'static str = "priorities/list/v1";
}

/// One priority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The entry's name, which stays the same when its title changes or it
    /// moves, unique across both horizons.
    #[schemars(with = "Slug")]
    pub id: String,
    /// What the priority is, in one line.
    #[schemars(with = "Summary")]
    pub title: String,
    /// The tasks and changes in this board that the priority covers. It
    /// may be empty when the work lives in another board.
    #[schemars(schema_with = "covers")]
    pub covers: Vec<String>,
    /// The day the owner last confirmed the entry. It is left out until
    /// the owner confirms it, and a change to its title or covers removes
    /// it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Date>")]
    pub confirmed: Option<String>,
}

/// `covers` names records in the board's own tree only, which
/// `x-board-local` makes `board check` hold it to.
fn covers(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "maxItems": MOST_COVERS,
        "items": {
            "type": "string",
            "format": "board-ref",
            "x-board-type": COVERED_TYPES,
            "x-board-local": true,
        },
    })
}

/// The two lists of a priorities record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub enum Horizon {
    Short,
    Long,
}

impl Horizon {
    pub const BOTH: [Self; 2] = [Self::Short, Self::Long];

    pub fn name(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Long => "long",
        }
    }
}

impl PrioritiesRecord {
    /// The entries of `horizon`, highest first.
    pub fn entries(&self, horizon: Horizon) -> &Vec<Entry> {
        match horizon {
            Horizon::Short => &self.short,
            Horizon::Long => &self.long,
        }
    }

    pub fn entries_mut(&mut self, horizon: Horizon) -> &mut Vec<Entry> {
        match horizon {
            Horizon::Short => &mut self.short,
            Horizon::Long => &mut self.long,
        }
    }

    /// The horizon and index of the entry named `id`.
    pub fn find(&self, id: &str) -> Option<(Horizon, usize)> {
        Horizon::BOTH.into_iter().find_map(|horizon| {
            (self.entries(horizon).iter())
                .position(|entry| entry.id == id)
                .map(|index| (horizon, index))
        })
    }
}
