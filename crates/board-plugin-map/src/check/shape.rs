//! Checks that a map's ramp, line kinds and tables agree with its items.

use std::collections::BTreeSet;

use board_ops::PluginError;

use super::refusal;
use crate::records::LayoutRecord;

/// Refuses a map whose items name a bin its ramp does not hold, whose line
/// kinds or tables repeat a name, or whose table rows do not fit their
/// columns or name an item the map does not hold.
pub(super) fn shaded_and_tabled(
    map: &LayoutRecord,
    items: &BTreeSet<&str>,
) -> Result<(), PluginError> {
    let bins = map.ramp.as_ref().map_or(0, |ramp| ramp.bins.len());
    for item in &map.items {
        if let Some(bin) = item.bin.filter(|bin| usize::from(*bin) >= bins) {
            return Err(refusal(
                "map::layout::bin_out_of_range",
                format!(
                    "the item {} has bin {bin}, and the map's ramp holds {bins} bins",
                    item.id
                ),
                "add a ramp with enough bins, or give the item a bin the ramp holds, counted from 0",
                [("id", item.id.as_str())],
            ));
        }
    }
    let mut kinds = BTreeSet::new();
    for kind in &map.line_kinds {
        if !kinds.insert(kind.kind.as_str()) {
            return Err(refusal(
                "map::layout::duplicate_id",
                format!("the map styles the line kind {} twice", kind.kind),
                "give each line kind one entry in line_kinds",
                [("id", kind.kind.as_str())],
            ));
        }
    }
    let mut tables = BTreeSet::new();
    for table in &map.tables {
        if !tables.insert(table.id.as_str()) {
            return Err(refusal(
                "map::layout::duplicate_id",
                format!("the map holds two tables named {}", table.id),
                "give each table a name of its own",
                [("id", table.id.as_str())],
            ));
        }
        for (at, row) in table.rows.iter().enumerate() {
            if row.cells.len() != table.columns.len() {
                return Err(refusal(
                    "map::layout::table_shape",
                    format!(
                        "row {} of the table {} has {} cells, and the table has {} columns",
                        at + 1,
                        table.id,
                        row.cells.len(),
                        table.columns.len()
                    ),
                    "give each row one cell for each column",
                    [("id", table.id.as_str())],
                ));
            }
            if let Some(item) = row.item.as_deref().filter(|id| !items.contains(id)) {
                return Err(refusal(
                    "map::layout::unknown_id",
                    format!(
                        "row {} of the table {} names the item {item}, which the map does not hold",
                        at + 1,
                        table.id
                    ),
                    "add it to the map, or name one the map holds",
                    [("id", item)],
                ));
            }
        }
    }
    Ok(())
}
