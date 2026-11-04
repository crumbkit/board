//! `add`: append text under a `## HH:MM` heading to today's daily note, to
//! a topic's note, or to a new note in the personal repo's inbox.

use std::time::{Duration, SystemTime};

use board_model::{Code, shown_remote};
use board_ops::{Host, PluginError};
use board_repos::{
    ATTEMPTS, Date, EDITED, MOVED, Repo, Writer, create, day_start, landed, refused, slug, today,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::route::choose;
use crate::{PLUGIN, Through, read, refusal, remote, trimmed, writer};

/// How many names an inbox note tries, `<time>.md`, then `<time>-2.md` and
/// on, when notes share a second.
const INBOX_NAMES: usize = 10;

/// Append text under a `## HH:MM` heading to today's daily note, or to a topic's note.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "AddInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = r"\S"),
        description = "The Markdown to append; not only whitespace."
    )]
    pub(crate) text: String,
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
    #[schemars(
        length(min = 1),
        description = "The title of the note to append to, at notes/<slug>.md; today's daily \
                       note when left out."
    )]
    pub(crate) topic: Option<String>,
}

/// Where the text went.
///
/// `path` is the logical path written and `commit` the repo's commit that
/// holds it: the mount's pin, or the board's commit for the board's own tree,
/// null when the note was left uncommitted.
/// `candidates` is null, or the notes repos covering the remote when the note
/// went to the personal repo's `inbox/` because not exactly one covers it.
/// `parked` is null, or the Board code that kept the text out of the note: it
/// went to the sibling `<name>.agent.md` instead. `unpushed` is null when the
/// commit reached the repo's remote, or the code of the sync that did not
/// push it: `notes::repo::not_synced` for the board's own tree.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "AddOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) commit: Option<String>,
    pub(crate) candidates: Option<Vec<String>>,
    pub(crate) parked: Option<String>,
    pub(crate) unpushed: Option<String>,
}

/// The local date and time of `now`, `offset` seconds east of UTC: the
/// calendar date, and the hours, minutes and seconds into it.
pub(crate) fn clock(now: SystemTime, offset: i32) -> (Date, u64, u64, u64) {
    let date = today(now, offset, Duration::ZERO);
    let seconds = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let into = u64::try_from(seconds.saturating_add(i64::from(offset)).rem_euclid(86_400))
        .unwrap_or_default();
    (date, into / 3600, into / 60 % 60, into % 60)
}

pub(crate) fn add(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input(PLUGIN, "add", input)?;
    let (name, topic) = (given.repo.as_deref(), given.topic.as_deref());
    let remote = remote(host, name, given.remote.as_deref());
    let choice = choose(host, name, remote.as_deref(), true)?;
    let repo = &choice.repo;
    let settings = host.settings();
    let (date, hours, minutes, seconds) = clock(settings.now, settings.utc_offset);
    let section = format!("## {hours:02}:{minutes:02}\n\n{}\n", trimmed(&given.text));
    let clones = Through(host);
    let writer = writer(host, &clones);
    let synced = repo.sync(writer, true)?;
    let mut parked = None;
    let (path, commit) = if let Some(candidates) = &choice.candidates {
        // An inbox note is a new file, which nothing else writes, so it is
        // never parked.
        let stem = format!("inbox/{date}-{hours:02}{minutes:02}{seconds:02}");
        let text = inbox(remote.as_deref(), candidates, topic) + &section;
        create(writer, repo, &stem, &text, INBOX_NAMES, "notes::note::busy")?
    } else {
        let start = day_start(settings.day_start.as_deref());
        let day = today(settings.now, settings.utc_offset, start);
        let mut path = topic.map_or_else(
            || format!("daily/{day}.md"),
            |topic| format!("notes/{}.md", slug(topic)),
        );
        let first = topic.map_or_else(String::new, |topic| format!("# {topic}\n\n"));
        let (why, mut commit) = match synced {
            Some(code) => (Some(code.as_str().to_owned()), None),
            None => append(writer, repo, &path, &first, &section)?,
        };
        if let Some(why) = why {
            parked = Some(why);
            path = format!("{}.agent.md", path.trim_end_matches(".md"));
            // The sibling takes no title, so Reflect never finds two notes
            // of one title.
            let (busy, written) = append(writer, repo, &path, "", &section)?;
            if busy.is_some() {
                let logical = repo.logical(&path);
                let why = format!(
                    "{logical} also changed after it was read, so the note was not added; call \
                     again, and if this persists, commit or discard the edit not committed in its \
                     clone"
                );
                return Err(refusal(
                    "notes::note::busy",
                    why,
                    [("path", json!(logical))],
                ));
            }
            commit = written;
        }
        (path, commit)
    };
    let (commit, unpushed) = landed(writer, repo, commit)?;
    let output = Output {
        repo: repo.root.clone(),
        path: repo.logical(&path),
        commit,
        candidates: choice.candidates,
        parked,
        unpushed,
    };
    Ok(json!(output))
}

/// Appends `section` to the note at `path`, starting a new one with
/// `first`. It returns why the section was not written, or none, and the
/// commit that holds the write, none when the board's own tree left it
/// uncommitted.
pub(crate) fn append(
    writer: Writer<'_>,
    repo: &Repo,
    path: &str,
    first: &str,
    section: &str,
) -> Result<(Option<String>, Option<String>), PluginError> {
    let mut moved = None;
    for _ in 0..ATTEMPTS {
        repo.pin(writer)?;
        let note = read(writer.host, repo, path)?;
        let text = match note.text.as_deref() {
            Some(text) if !text.is_empty() => {
                format!("{}\n\n{section}", text.trim_end_matches('\n'))
            }
            _ => format!("{first}{section}"),
        };
        let error = match repo.write(writer.host, path, &text, note.blob.as_deref()) {
            Ok(commit) => return Ok((None, commit)),
            Err(error) => error,
        };
        let code = refused(&error);
        if code == Code::PutHeadMoved && repo.clone.is_none() {
            return Ok((None, None));
        }
        if code == EDITED {
            return Ok((Some(EDITED.as_str().to_owned()), None));
        }
        if !MOVED.contains(&code) {
            return Err(PluginError::board_refused(PLUGIN, "put", &error.refusal()));
        }
        moved = Some(code);
    }
    Ok((moved.map(|code| code.as_str().to_owned()), None))
}

/// The frontmatter of a note in the inbox: the remote it came from, the
/// repos it may belong to, and its topic, each value written as Python's
/// `json.dumps` writes it, which is YAML too.
pub(crate) fn inbox(remote: Option<&str>, candidates: &[String], topic: Option<&str>) -> String {
    let string = |text: &str| Value::from(text).to_string();
    let listed: Vec<String> = candidates.iter().map(|each| string(each)).collect();
    let mut lines = Vec::new();
    if let Some(remote) = remote {
        lines.push(format!("remote: {}", string(&shown_remote(remote))));
    }
    lines.push(format!("candidates: [{}]", listed.join(", ")));
    if let Some(topic) = topic {
        lines.push(format!("topic: {}", string(topic)));
    }
    format!("---\n{}\n---\n\n", lines.join("\n"))
}
