//! The map plugin built into Board (docs/plugins/built-in/map.md). It ships
//! `map/layout/v1`, one map's sheet, regions, items, lines and notes, and
//! `map.check`, which lists what a reader of the map would see as broken.

pub mod check;
mod geom;
pub mod records;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use crate::records::LayoutRecord;

#[derive(Clone, Copy, Debug)]
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (LayoutRecord::TYPE.parse())
            .expect("LayoutRecord's TYPE is a record type; a test parses it");
        vec![(name, layout_schema())]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "check" => check::call(host, &input),
            other => Err(PluginError {
                code: "map::call::unknown_capability".to_owned(),
                message: format!("map offers no capability {other}; it offers check"),
                fields: Map::new(),
                hint: None,
            }),
        }
    }
}

fn manifest() -> Value {
    let settings = SchemaSettings::draft2020_12;
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "map",
        "version": "0.1.0",
        "description": "A map draws items as marks with labels, in titled regions, joined by typed \
            lines, with notes anchored to an item or a line, from one map/layout/v1 record at \
            /maps/<name>. `board call map.check --path /maps/<name>` lists what a reader would see \
            as broken. docs/plugins/built-in/map.md in Board's repository says more.",
        "metadata": {"area": "planning"},
        "types": [LayoutRecord::TYPE],
        "capabilities": [{
            "name": "check",
            "description": "What a reader would see as broken in a map: labels over labels, marks \
                or region titles, labels off the sheet or outside their region, lines through \
                labels or marks, lines leaving an item at nearly one angle, overlapping marks, and \
                labels or notes nearer another mark or line than their own.",
            "inputSchema": schema_of::<check::Input>(settings()),
            "outputSchema": schema_of::<check::Output>(settings().for_serialize()),
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            },
            "_meta": {},
        }],
    })
}

/// The schema `LayoutRecord` generates.
fn layout_schema() -> Value {
    schema_of::<LayoutRecord>(SchemaSettings::draft2020_12())
}

#[cfg(test)]
mod tests;
