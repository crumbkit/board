//! The `migrate` capability: rewrites change records written before the
//! split of `changes/change/v1` (docs/plugins/lifecycle/updating.md).
//!
//! A change of the old shape keeps its entity fields. A prototype's fields
//! move to a `changes/prototype/v1` beside it, the records it lists move to
//! its newest plan, and a GitHub pull request or issue URL becomes the
//! github plugin's record of it, which `github.ref` writes. The old
//! `/changes/sync` stamp moves to Board's cache. Everything else in the
//! board is left as it is.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

use board_model::parse_json;
use board_ops::{Host, Plugin, PluginError, literal};
use board_plugin::shape::{Record, Tag, Url};
use board_validate::Schema;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::board::{self, rows, text};
use crate::change::{OWN, prototypes, record_sync};
use crate::plan::stamp;
use crate::records::{ChangeRecord, PlanRecord, PrototypeRecord, UnderstandingRecord};
use crate::{PLUGIN, to_value};

/// A GitHub pull request or issue page, as the Python plugin stored it:
/// owner, repository, `pull` or `issues`, and number. Only this migration
/// reads such a URL; it goes when no board holds the old shape.
static GITHUB_PAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/(pull|issues)/([0-9]+)/?$",
    )
    .expect("the pattern is a constant; the migrate tests compile it")
});
/// Where the Python plugin kept the day of the last sync.
const SYNC_PATH: &str = "/changes/sync";
const SYNC_TYPE: &str = "changes/sync/v1";
/// The fields of the new shape. An old `plan` names a Markdown file, which
/// [`olds`] tells apart.
const KEPT: [&str; 14] = [
    "$type",
    "title",
    "mount",
    "phase",
    "plan",
    "route",
    "task",
    "head",
    "tickets",
    "pulls",
    "touched",
    "over_limit",
    "merge_commit_url",
    "dropped_reason",
];
/// The most rewritten paths the output lists (STANDARDS 23).
const LISTED: usize = 100;

/// Whether to write the migration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MigrateInput")]
pub(crate) struct Input {
    #[serde(default)]
    #[schemars(description = "Write every rewritten record; without it nothing is written.")]
    apply: bool,
}

/// A record the migration could not rewrite, and why, with the GitHub page
/// `github.ref` could not record when that is the reason.
#[derive(Debug, Serialize, JsonSchema)]
struct Left {
    path: String,
    why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Url>")]
    page: Option<String>,
}

/// What the migration rewrote or would rewrite.
///
/// `changes` lists the rewritten changes, at most 100, with `total` and
/// `truncated`. `plans` and `prototypes` count the records written. `refs`
/// lists each pull request or issue that `github.ref` records, as
/// owner/repo#number. `left` lists the changes and understanding checks
/// left as they are, which the owner fixes by hand. `synced` is the day moved from `/changes/sync` to
/// Board's cache, and `unrecorded` says why the cache did not take it, when
/// it did not.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "MigrateOutput")]
pub(crate) struct Output {
    applied: bool,
    changes: Vec<String>,
    total: usize,
    truncated: bool,
    plans: usize,
    prototypes: usize,
    refs: Vec<String>,
    left: Vec<Left>,
    synced: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unrecorded: Option<String>,
}

/// One old change: its path, blob and body.
struct Old {
    path: String,
    blob: String,
    body: Map<String, Value>,
}

/// The change records of the board's own whose body still holds a field of
/// the old shape.
fn olds(host: &dyn Host) -> Result<Vec<Old>, PluginError> {
    let sql = format!(
        "SELECT logical, blob, body FROM records WHERE type = {} AND substr(logical, 1, {}) = {} \
         ORDER BY logical",
        literal(ChangeRecord::TYPE),
        OWN.len(),
        literal(OWN)
    );
    let mut found = Vec::new();
    for row in rows(host, &sql)? {
        let parsed = text(row.get(2)).and_then(|body| parse_json(body.as_bytes()).ok());
        let (Some(path), Some(blob), Some(Value::Object(body))) =
            (text(row.first()), text(row.get(1)), parsed)
        else {
            continue;
        };
        if body.keys().any(|key| !KEPT.contains(&key.as_str())) || old_plan(&body) {
            found.push(Old {
                path: path.to_owned(),
                blob: blob.to_owned(),
                body,
            });
        }
    }
    Ok(found)
}

/// Whether `body`'s `plan` names Markdown, as the old shape did.
fn old_plan(body: &Map<String, Value>) -> bool {
    (body.get("plan").and_then(Value::as_str))
        .is_some_and(|plan| Path::new(plan).extension() == Some(OsStr::new("md")))
}

/// What the migration reads once for the whole board: every plan record
/// under `/changes/`, by its change and then by path, with its blob, which
/// of the Markdown plans the old changes name are there, and the changes
/// that already have something at `<path>/prototype`.
struct Known {
    plans: BTreeMap<String, BTreeMap<String, (PlanRecord, String)>>,
    files: BTreeSet<String>,
    prototypes: BTreeSet<String>,
}

fn known(host: &dyn Host, olds: &[Old]) -> Result<Known, PluginError> {
    let sql = format!(
        "SELECT logical, blob, body FROM records WHERE type = {} AND substr(logical, 1, {}) = {}",
        literal(PlanRecord::TYPE),
        OWN.len(),
        literal(OWN)
    );
    let mut plans: BTreeMap<String, BTreeMap<String, (PlanRecord, String)>> = BTreeMap::new();
    for row in rows(host, &sql)? {
        let parsed = (text(row.get(2)))
            .and_then(|body| parse_json(body.as_bytes()).ok())
            .and_then(|value| serde_json::from_value::<PlanRecord>(value).ok());
        if let (Some(logical), Some(blob), Some(plan)) =
            (text(row.first()), text(row.get(1)), parsed)
            && let Some((change, _)) = logical.rsplit_once("/plans/")
        {
            (plans.entry(change.to_owned()).or_default())
                .insert(logical.to_owned(), (plan, blob.to_owned()));
        }
    }
    let named: BTreeSet<String> = (olds.iter())
        .filter_map(|old| string(&old.body, "plan"))
        .filter(|named| !named.is_empty())
        .collect();
    let mut files = BTreeSet::new();
    if !named.is_empty() {
        let listed: Vec<String> = named.iter().map(|each| literal(each)).collect();
        let sql = format!(
            "SELECT logical FROM entries WHERE logical IN ({})",
            listed.join(", ")
        );
        for row in rows(host, &sql)? {
            if let Some(logical) = text(row.first()) {
                files.insert(logical.to_owned());
            }
        }
    }
    let prototypes = prototypes(host)?.into_keys().collect();
    Ok(Known {
        plans,
        files,
        prototypes,
    })
}

/// The understanding checks of the old shape, which the migration cannot
/// rewrite, since a question of that shape holds no answer of the design's.
/// Each is left with why, for the owner to fix by hand.
fn old_checks(host: &dyn Host) -> Result<Vec<Left>, PluginError> {
    let sql = format!(
        "SELECT logical, body FROM records WHERE type = {} AND substr(logical, 1, {}) = {} \
         ORDER BY logical",
        literal(UnderstandingRecord::TYPE),
        OWN.len(),
        literal(OWN)
    );
    let mut left = Vec::new();
    for row in rows(host, &sql)? {
        let parsed = text(row.get(1)).and_then(|body| parse_json(body.as_bytes()).ok());
        let (Some(path), Some(body)) = (text(row.first()), parsed) else {
            continue;
        };
        if serde_json::from_value::<UnderstandingRecord>(body).is_err() {
            left.push(Left {
                page: None,
                path: path.to_owned(),
                why: "it is an understanding check of the old shape; give each question the \
                      design's answer in `design`, and keep only the fields \
                      changes/understanding/v1 has"
                    .to_owned(),
            });
        }
    }
    Ok(left)
}

/// The owner/repo#number a GitHub page names, and whether it is a pull
/// request, or none for any other URL.
fn github_ref(url: &str) -> Option<(String, bool)> {
    let captures = GITHUB_PAGE.captures(url)?;
    let reference = format!("{}/{}#{}", &captures[1], &captures[2], &captures[4]);
    Some((reference, &captures[3] == "pull"))
}

/// The strings of `body`'s list `key`.
fn strings(body: &Map<String, Value>, key: &str) -> Vec<String> {
    (body
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|each| each.as_str().map(str::to_owned))
    .collect()
}

fn string(body: &Map<String, Value>, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Adds `each` to `listed` once.
fn add(listed: &mut Vec<String>, each: String) {
    if !listed.contains(&each) {
        listed.push(each);
    }
}

/// What one old change becomes.
#[derive(Default)]
struct Plan {
    lines: Vec<Value>,
    refs: Vec<Page>,
    plans: usize,
    prototype: bool,
}

/// The lines that rewrite `old`, and the GitHub pages it names, or why it
/// cannot be rewritten.
fn rewrite(known: &Known, old: &Old, record: &str) -> Result<Plan, String> {
    let body = &old.body;
    let moved = [
        "records",
        "issue",
        "pull_request",
        "kind",
        "question",
        "state",
        "verdict",
        "verdict_artifact",
    ];
    let unknown: Vec<&str> = (body.keys())
        .map(String::as_str)
        .filter(|key| !KEPT.contains(key) && !moved.contains(key))
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "it holds fields no shape of changes/change/v1 has: {}",
            unknown.join(", ")
        ));
    }
    let mut plan = Plan::default();
    for key in ["pull_request", "issue"] {
        if let Some(url) = string(body, key) {
            let Some(found) = github_ref(&url) else {
                return Err(format!(
                    "its {key} {url} is not a GitHub page, so no github record can stand for it"
                ));
            };
            plan.refs.push(Page {
                reference: found.0,
                pull: found.1,
                url,
            });
        }
    }
    let mut change = body.clone();
    change.retain(|key, _| KEPT.contains(&key.as_str()));
    let path = &old.path;
    let none = BTreeMap::new();
    let plans = known.plans.get(path).unwrap_or(&none);
    let named = string(body, "plan");
    let held = (named.as_deref())
        .and_then(|named| (plans.iter()).find(|(at, (plan, _))| *at == named || plan.text == named))
        .or_else(|| plans.iter().next_back())
        .map(|(at, (plan, blob))| (at.clone(), plan_copy(plan), blob.clone()));
    let records = strings(body, "records");
    let (target, mut kept, expect) = match held {
        Some(found) => found,
        None => fresh_plan(known, old, record, named, &mut plan.lines),
    };
    if !records.is_empty() || expect == "none" {
        let mut listed = kept.records.take().unwrap_or_default();
        for each in records {
            add(&mut listed, each);
        }
        kept.records = (!listed.is_empty()).then_some(listed);
        plan.plans += 1;
        plan.lines
            .push(json!({"path": target, "expect": expect, "record": kept}));
    }
    change.insert("plan".to_owned(), json!(target));
    // A prototype's state is its change's phase, so only the question and
    // the verdict move. A prototype record already there is kept.
    if string(body, "kind").as_deref() == Some("prototype") && !known.prototypes.contains(path) {
        let prototype = PrototypeRecord {
            tag: Tag::default(),
            question: string(body, "question")
                .or_else(|| string(body, "title"))
                .unwrap_or_default(),
            verdict: string(body, "verdict"),
            verdict_artifact: string(body, "verdict_artifact"),
        };
        plan.lines.push(
            json!({"path": format!("{path}/prototype"), "expect": "none", "record": prototype}),
        );
        plan.prototype = true;
    }
    plan.lines
        .push(json!({"path": path, "expect": old.blob, "change": change}));
    Ok(plan)
}

/// A new plan record at `{change}/plans/{record}`, with its text the old
/// plan file when that file exists, or else a new file holding the title.
fn fresh_plan(
    known: &Known,
    old: &Old,
    record: &str,
    named: Option<String>,
    lines: &mut Vec<Value>,
) -> (String, PlanRecord, String) {
    let at = format!("{}/plans/{record}", old.path);
    let existing = named.filter(|named| known.files.contains(named));
    let text = existing.unwrap_or_else(|| {
        let written = format!("{at}.md");
        let title = string(&old.body, "title").unwrap_or_default();
        lines.push(json!({"path": written, "expect": "none", "text": format!("# {title}\n")}));
        written
    });
    let fresh = PlanRecord {
        tag: Tag::default(),
        text,
        uses: Vec::new(),
        paths: None,
        records: None,
    };
    (at, fresh, "none".to_owned())
}

/// A copy of `plan`, since a record's tag is not `Clone`.
fn plan_copy(plan: &PlanRecord) -> PlanRecord {
    PlanRecord {
        tag: Tag::default(),
        text: plan.text.clone(),
        uses: plan.uses.clone(),
        paths: plan.paths.clone(),
        records: plan.records.clone(),
    }
}

/// The day the old sync stamp holds, and its blob, or none when there is
/// none.
fn old_sync(host: &dyn Host) -> Result<Option<(String, String)>, PluginError> {
    let sql = format!(
        "SELECT blob, json_extract(body, '$.synced') FROM records WHERE logical = {} AND type = {}",
        literal(SYNC_PATH),
        literal(SYNC_TYPE)
    );
    let found = rows(host, &sql)?;
    Ok(found
        .first()
        .and_then(|row| Some((text(row.get(1))?.to_owned(), text(row.first())?.to_owned()))))
}

/// A GitHub page an old change names: its `owner/repo#number`, whether it
/// is a pull request, and its URL.
struct Page {
    reference: String,
    pull: bool,
    url: String,
}

/// Why `github.ref` did not record a page: the page itself, which leaves
/// its change as it is, or Board, which stops the migration.
enum Unrecorded<'a> {
    Page(&'a Page, String),
    Board(PluginError),
}

/// The path of the github record `github.ref` writes for each page, or
/// with no `apply` none. A refusal of the github plugin's own, such as a
/// page GitHub does not have, names the page; any other refusal, such as
/// the plugin not being enabled, stops the migration.
fn recorded<'a>(
    host: &dyn Host,
    apply: bool,
    pages: &'a [Page],
) -> Result<Vec<Option<String>>, Unrecorded<'a>> {
    let mut paths = Vec::new();
    for page in pages {
        if !apply {
            paths.push(None);
            continue;
        }
        let output =
            match board::call_or_refusal(host, "github.ref", &json!({"ref": page.reference})) {
                Ok(output) => output,
                Err(refusal) if refusal.code.as_str().starts_with("github::") => {
                    let why = format!(
                        "github.ref could not record {}: {}: {}; fix or remove the page, then run \
                     migrate again",
                        page.url,
                        refusal.code.as_str(),
                        refusal.message
                    );
                    return Err(Unrecorded::Page(page, why));
                }
                Err(refusal) => return Err(Unrecorded::Board(board::refused("call", &refusal))),
            };
        match string(output.as_object().unwrap_or(&Map::new()), "path") {
            Some(at) if !at.is_empty() => paths.push(Some(at)),
            _ => {
                let why = format!(
                    "github.ref recorded {} but named no record path; run migrate again",
                    page.url
                );
                return Err(Unrecorded::Page(page, why));
            }
        }
    }
    Ok(paths)
}

/// The plugin's own schemas, compiled, by type.
fn compiled() -> BTreeMap<String, Schema> {
    (crate::Changes.schemas().into_iter())
        .filter_map(|(kind, document)| {
            Some((kind.as_str().to_owned(), Schema::compile(document).ok()?))
        })
        .collect()
}

/// Whether every record among `lines` passes its type's schema, so a dry
/// run that lists a change promises an apply that writes it. A record of a
/// type the plugin does not ship is left to Board's own check.
fn conforms(schemas: &BTreeMap<String, Schema>, lines: &[Value]) -> Result<(), String> {
    for line in lines {
        let Some(record) = line.get("record").or_else(|| line.get("change")) else {
            continue;
        };
        let kind = record
            .get("$type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(schema) = schemas.get(kind)
            && let Err(error) = schema.validate(record)
        {
            let at = line.get("path").and_then(Value::as_str).unwrap_or_default();
            return Err(format!(
                "its rewritten record at {at} {error}, so nothing of it is written; fix the old \
                 change and run migrate again"
            ));
        }
    }
    Ok(())
}

/// Adds the github records' paths to the rewritten change, the last of
/// `written`, and makes it the line's record.
fn finish(written: &mut [Value], pulls: Vec<String>, tickets: Vec<String>) {
    let Some(last) = written.last_mut().and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(Value::Object(change)) = last.get_mut("change") {
        for (key, found) in [("pulls", pulls), ("tickets", tickets)] {
            let mut listed = strings(change, key);
            for each in found {
                add(&mut listed, each);
            }
            if !listed.is_empty() {
                change.insert(key.to_owned(), json!(listed));
            }
        }
    }
    if let Some(change) = last.remove("change") {
        last.insert("record".to_owned(), change);
    }
}

/// Rewrites each old change, or with no `apply` says what it would write.
///
/// With `apply`, `github.ref` writes each GitHub record first, each in its
/// own commit, and then every rewritten record is written in one commit.
pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { apply } = PluginError::read_input(PLUGIN, "migrate", input)?;
    let record = stamp(host.settings().now);
    let mut changes = Vec::new();
    let mut left = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    let (mut plans, mut prototypes) = (0, 0);
    let mut lines = Vec::new();
    let olds = olds(host)?;
    let known = known(host, &olds)?;
    let schemas = compiled();
    for old in olds {
        let planned = match rewrite(&known, &old, &record) {
            Ok(planned) => planned,
            Err(why) => {
                left.push(Left {
                    path: old.path.clone(),
                    why,
                    page: None,
                });
                continue;
            }
        };
        let Plan {
            lines: mut written,
            refs: pages,
            plans: plan_count,
            prototype,
        } = planned;
        // Checked before github.ref writes anything for the change.
        if let Err(why) = conforms(&schemas, &written) {
            left.push(Left {
                path: old.path.clone(),
                why,
                page: None,
            });
            continue;
        }
        let (mut pulls, mut tickets) = (Vec::new(), Vec::new());
        match recorded(host, apply, &pages) {
            Ok(paths) => {
                for (page, at) in pages.iter().zip(paths) {
                    add(&mut refs, page.reference.clone());
                    if page.pull {
                        pulls.extend(at);
                    } else {
                        tickets.extend(at);
                    }
                }
            }
            Err(Unrecorded::Page(page, why)) => {
                left.push(Left {
                    path: old.path.clone(),
                    why,
                    page: Some(page.url.clone()),
                });
                continue;
            }
            Err(Unrecorded::Board(error)) => return Err(error),
        }
        finish(&mut written, pulls, tickets);
        plans += plan_count;
        prototypes += usize::from(prototype);
        changes.push(old.path.clone());
        lines.extend(written);
    }
    left.extend(old_checks(host)?);
    let sync = old_sync(host)?;
    if let Some((_, blob)) = &sync {
        lines.push(json!({"path": SYNC_PATH, "expect": blob, "remove": true}));
    }
    let mut unrecorded = None;
    if apply {
        if !lines.is_empty() {
            board::put(host, &lines)?;
        }
        if let Some((day, _)) = &sync {
            unrecorded = record_sync(host, day)?;
        }
    }
    let total = changes.len();
    changes.truncate(LISTED);
    Ok(to_value(Output {
        applied: apply,
        changes,
        total,
        truncated: total > LISTED,
        plans,
        prototypes,
        refs,
        left,
        synced: sync.map(|(day, _)| day),
        unrecorded,
    }))
}

#[cfg(test)]
mod tests {
    use super::github_ref;

    #[test]
    fn a_github_pull_request_or_issue_page_names_its_reference_and_another_url_none() {
        assert_eq!(
            github_ref("https://github.com/crumbkit/board/pull/12/"),
            Some(("crumbkit/board#12".to_owned(), true))
        );
        assert_eq!(
            github_ref("https://github.com/o/r/issues/3"),
            Some(("o/r#3".to_owned(), false))
        );
        assert_eq!(
            github_ref("https://gitlab.com/o/r/-/merge_requests/1"),
            None
        );
    }
}
