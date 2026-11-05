//! The topics a migration creates: one `topics/topic/v1` at
//! `/topics/<slug>` for each name a subject held that is neither a
//! repository nor a board path, so the subject can name the topic by path.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

/// The topic type's committed schema, which a `--map` topic must pass.
pub(crate) const SCHEMA: &str =
    include_str!("../../../crates/board-plugin-topics/types/topics/topic/v1.schema.json");

/// The topic type.
pub(crate) const TYPE: &str = "topics/topic/v1";

/// The slug of `name`: its ASCII letters and digits in lowercase, each run
/// of anything else one `-`, trimmed. `Raycast Workspaces` is
/// `raycast-workspaces`. None when nothing is left.
pub(crate) fn slug(name: &str) -> Option<String> {
    let mut slug = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    (!slug.is_empty()).then_some(slug)
}

/// The slug a subject that names a topic, `/topics/<slug>`, holds.
pub(crate) fn named(subject: &str) -> Option<&str> {
    subject
        .strip_prefix("/topics/")
        .filter(|slug| self::slug(slug).as_deref() == Some(*slug))
}

/// The topics a run gathers: each slug, the names that led to it and the
/// record a `--map` entry gave for it.
#[derive(Debug, Default)]
pub(crate) struct Gathered {
    topics: BTreeMap<String, (BTreeSet<String>, Option<Value>)>,
}

impl Gathered {
    /// Records that `name` led to the topic at `slug`, as `given` says when
    /// a `--map` entry gave the topic.
    pub(crate) fn add(&mut self, slug: &str, name: &str, given: Option<&Value>) {
        let (names, record) = self.topics.entry(slug.to_owned()).or_default();
        names.insert(name.to_owned());
        if record.is_none() {
            *record = given.cloned();
        }
    }

    /// Each topic's slug, record and the names that led to it. A topic a `--map` entry gave keeps its
    /// own fields. Otherwise its title is the name that is the slug, or one
    /// whose slug is the topic's, or else the first name in order, and the other names are
    /// its aliases; a given topic gains them as aliases too.
    pub(crate) fn records(&self) -> Vec<(String, Value, Vec<String>)> {
        (self.topics.iter())
            .map(|(slug, (names, given))| {
                let title = (given.as_ref())
                    .and_then(|given| given.get("title")?.as_str().map(str::to_owned))
                    .or_else(|| names.iter().find(|name| *name == slug).cloned())
                    .or_else(|| {
                        (names.iter())
                            .find(|name| self::slug(name).as_deref() == Some(slug))
                            .cloned()
                    })
                    .or_else(|| names.first().cloned())
                    .unwrap_or_else(|| slug.clone());
                let mut record = match given {
                    Some(Value::Object(given)) => given.clone(),
                    _ => Map::from_iter([
                        ("$type".to_owned(), json!(TYPE)),
                        ("title".to_owned(), json!(title)),
                    ]),
                };
                let mut aliases: Vec<String> = (record.get("aliases").and_then(Value::as_array))
                    .map(|aliases| {
                        aliases
                            .iter()
                            .filter_map(|alias| alias.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                for name in names {
                    if *name != title && !aliases.contains(name) {
                        aliases.push(name.clone());
                    }
                }
                if !aliases.is_empty() {
                    record.insert("aliases".to_owned(), json!(aliases));
                }
                (
                    slug.clone(),
                    Value::Object(record),
                    names.iter().cloned().collect(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
