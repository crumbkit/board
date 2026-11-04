//! The notes plugin built into Board (plugins/notes/README.md): it appends
//! to and reads notes in notes repos in Reflect's layout, which a board
//! holds or mounts. Routing and the append loop are its own; reading,
//! writing and the write-and-retry loop are board-repos'.

mod add;
mod daily;
pub mod records;
mod route;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Origin, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use board_repos::{Repo, Through, Writer};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::RepoRecord;

/// The plugin's name, which its codes start with.
const PLUGIN: &str = "notes";

#[derive(Clone, Copy, Debug)]
pub struct Notes;

/// Each capability's name and description, in the order the manifest
/// lists them.
const CAPABILITIES: [(&str, &str); 2] = [
    (
        "add",
        "Sync the notes repo, append text under a `## HH:MM` heading to today's daily note or a \
         topic's note, commit it and push it.",
    ),
    (
        "daily",
        "Sync the notes repo and print a day's daily note, unless the note is private.",
    ),
];

impl Plugin for Notes {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let name = (RepoRecord::TYPE.parse()).expect("TYPE is a record type; a test parses it");
        vec![(
            name,
            schema_of::<RepoRecord>(SchemaSettings::draft2020_12()),
        )]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        match capability.as_str() {
            "add" => add::add(host, &input),
            "daily" => daily::daily(host, &input),
            other => Err(refusal(
                "notes::call::unknown_capability",
                format!("notes offers no capability {other}; it offers add and daily"),
                [],
            )),
        }
    }
}

/// A refusal of the plugin's own, with `fields`.
pub(crate) fn refusal<const N: usize>(
    code: &str,
    message: String,
    fields: [(&str, Value); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: (fields.into_iter())
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Map<_, _>>(),
        hint: None,
    }
}

/// What a call writes through: the host, its clones, and git's deadline on
/// each sync and pin.
fn writer<'a>(host: &'a dyn Host, clones: &'a Through<'a>) -> Writer<'a> {
    Writer {
        host,
        clones,
        plugin: PLUGIN,
        deadline: board_tree::GIT_DEADLINE,
    }
}

/// The note at `path` in `repo`, at the pin, refused when it is not UTF-8
/// text.
fn read(host: &dyn Host, repo: &Repo, path: &str) -> Result<board_repos::File, PluginError> {
    let note = repo.read(host, PLUGIN, path)?;
    if note.text.is_none() && note.blob.is_some() {
        let logical = repo.logical(path);
        let why = format!(
            "{logical} is not UTF-8 text, so no section can be appended to it; nothing was written"
        );
        return Err(refusal(
            "notes::note::not_text",
            why,
            [("path", json!(logical))],
        ));
    }
    Ok(note)
}

/// The remote to route by: `remote`, else the caller's origin; none when
/// `repo` names the repo, since then no remote is read.
fn remote(host: &dyn Host, repo: Option<&str>, remote: Option<&str>) -> Option<String> {
    if repo.is_some() {
        return None;
    }
    remote
        .map(str::to_owned)
        .or_else(|| match host.caller_remote() {
            Origin::Url(url) => Some(url),
            Origin::Outside | Origin::Unread(_) => None,
        })
}

fn manifest() -> Value {
    let input = SchemaSettings::draft2020_12;
    let output = || SchemaSettings::draft2020_12().for_serialize();
    let schemas = [
        (
            schema_of::<add::Input>(input()),
            schema_of::<add::Output>(output()),
        ),
        (
            schema_of::<daily::Input>(input()),
            schema_of::<daily::Output>(output()),
        ),
    ];
    let capabilities: Vec<Value> = (CAPABILITIES.iter().zip(schemas))
        .map(|((name, description), (input, output))| {
            json!({
                "name": name,
                "description": description,
                "inputSchema": input,
                "outputSchema": output,
                "_meta": {},
            })
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": PLUGIN,
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Append a note to today's daily note, or to a topic's note, in a notes \
            repo in Reflect's layout, and read a day's daily note. Use it when the person asks \
            you to note something down or asks what their daily note says. Start with `board -C \
            <board> call notes.daily`, run in the repository you work in, which syncs the notes \
            repo that repository's git remote routes to and prints today's note. SKILL.md and \
            README.md in the plugin's directory say more.",
        "metadata": {"area": "notes"},
        "types": [RepoRecord::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;
