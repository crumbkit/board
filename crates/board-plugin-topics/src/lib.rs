//! The topics plugin built into Board (docs/plugins/built-in/topics.md).
//! It ships one record type and no capability: `evals.about` reads topics
//! to find evals by a topic's name.

pub mod records;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use crate::records::TopicRecord;

#[derive(Clone, Copy, Debug)]
pub struct Topics;

impl Plugin for Topics {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest())
            .expect("the manifest is built from constants; a test parses it")
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (TopicRecord::TYPE.parse())
            .expect("the topic's TYPE is a record type; a test parses it");
        vec![(name, topic_schema())]
    }

    fn call(
        &self,
        _host: &dyn Host,
        capability: &CapabilityName,
        _input: Value,
    ) -> Result<Value, PluginError> {
        Err(PluginError {
            code: "topics::call::unknown_capability".to_owned(),
            message: format!(
                "topics offers no capability {capability}; it only ships topics/topic/v1, which \
                 `board put /topics/<slug>` writes"
            ),
            fields: Map::new(),
            hint: None,
        })
    }
}

fn manifest() -> Value {
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "topics",
        "version": "0.1.0",
        "description": "A topic is a concept, tool, model or design that several records concern, at \
            /topics/<slug>, with its title, aliases and links. An eval or suite names it as a subject \
            by its path, and `board call evals.about --subject <name>` finds it by title or alias. \
            docs/plugins/built-in/topics.md in Board's repository says more.",
        "metadata": {"area": "testing"},
        "types": [TopicRecord::TYPE],
    })
}

/// The schema `TopicRecord` generates.
fn topic_schema() -> Value {
    schema_of::<TopicRecord>(SchemaSettings::draft2020_12())
}

#[cfg(test)]
mod tests;
