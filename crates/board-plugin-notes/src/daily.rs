//! `daily`: the day's daily note in the repo the caller's remote routes
//! to, unless it is private. A read creates no note.

use board_ops::{Host, PluginError};
use board_repos::{day_start, today};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::route::choose;
use crate::{PLUGIN, Through, python_space, read, remote, writer};

/// The values of `private` that plainly show a note; any other value hides
/// it.
const FALSE: [&str; 4] = ["false", "no", "off", "0"];

/// The daily note of one day, as the repo's remote holds it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "DailyInput")]
pub(crate) struct Input {
    #[schemars(
        description = "The notes repo: / for the board's own tree, or a mount's path \
                              such as /repos/notes-personal; chosen by the caller's git remote \
                              when left out."
    )]
    pub(crate) repo: Option<String>,
    #[schemars(
        length(min = 1),
        description = "A git remote, or owner/name, to route by in place of the origin of the \
                       directory board was started in."
    )]
    pub(crate) remote: Option<String>,
    #[serde(default)]
    #[schemars(
        schema_with = "date",
        description = "The day, as YYYY-MM-DD; today when left out, by the caller's time zone \
                       and day_start."
    )]
    pub(crate) date: Option<String>,
}

/// `date`: a day, which Board's validator checks as JSON Schema's `date`
/// format.
fn date(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "format": "date"})
}

/// The daily note.
///
/// `text` is null when the note does not exist, or when it is private:
/// its frontmatter has a `private` key whose value is not plainly false
/// (`false`, `no`, `off` or `0`), as Reflect keeps private notes from AI. A read
/// creates no note. `commit` is the repo's commit read, null for the board's
/// own tree, which is read from its working tree. `unsynced` is null when the
/// repo synced first, or the code of the sync that failed, and the note is
/// then as this clone holds it: `notes::repo::not_synced` for the board's own
/// tree.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "DailyOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) private: bool,
    pub(crate) text: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) unsynced: Option<String>,
}

pub(crate) fn daily(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "daily", input)?;
    let name = given.repo.as_deref();
    let remote = remote(host, name, given.remote.as_deref());
    let repo = choose(host, name, remote.as_deref(), false)?.repo;
    let settings = host.settings();
    let date = given.date.unwrap_or_else(|| {
        let start = day_start(settings.day_start.as_deref());
        today(settings.now, settings.utc_offset, start).to_string()
    });
    let path = format!("daily/{date}.md");
    let clones = Through(host);
    let writer = writer(host, &clones);
    let unsynced = match &repo.clone {
        None => Some(format!("{PLUGIN}::repo::not_synced")),
        Some(_) => (repo.sync(writer, false)?).map(|code| code.as_str().to_owned()),
    };
    let commit = repo.pin(writer)?;
    let note = read(host, &repo, &path)?;
    let hidden = note.text.as_deref().is_some_and(private);
    let output = Output {
        repo: repo.root.clone(),
        path: repo.logical(&path),
        exists: note.text.is_some(),
        private: hidden,
        text: if hidden { None } else { note.text },
        commit,
        unsynced,
    };
    Ok(json!(output))
}

/// Whether the note may be private: its frontmatter has a `private` key
/// whose value is not plainly false.
///
/// This reads the frontmatter's lines, not its YAML, and fails closed: a
/// quoted key, a tag such as `!!bool`, or a second `private` line hides the
/// note, so it may hide one that Reflect shows and never shows one that
/// Reflect hides. Lines end as Python's `str.splitlines` ends them.
pub(crate) fn private(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = python_lines(text);
    let strip = |text: &str| text.trim_matches(python_space).to_owned();
    if lines.next().map(|line| line.trim_end_matches(python_space)) != Some("---") {
        return false;
    }
    let mut hidden = false;
    for line in lines {
        if line.trim_end_matches(python_space) == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = strip(strip(key).trim_matches(['\'', '"']));
        if key == "private" {
            let value = value.split(" #").next().unwrap_or_default();
            let flag = strip(strip(value).trim_matches(['\'', '"'])).to_lowercase();
            hidden = hidden || !FALSE.contains(&flag.as_str());
        }
    }
    hidden
}

/// The lines of `text` as Python's `str.splitlines` gives them: split at
/// `\n`, `\r`, `\r\n` and the other line boundaries Python counts.
fn python_lines(text: &str) -> impl Iterator<Item = &str> {
    let boundary = |c: char| {
        matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    };
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(at) = rest.find(boundary) else {
            let line = rest;
            rest = "";
            return Some(line);
        };
        let line = &rest[..at];
        let after = &rest[at..];
        let width = if after.starts_with("\r\n") {
            2
        } else {
            after.chars().next().map_or(1, char::len_utf8)
        };
        rest = &after[width..];
        Some(line)
    })
}
