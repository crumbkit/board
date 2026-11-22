//! `to`: copy a record and everything a reader needs into a mounted board.
//!
//! `closure` finds the set. A typed reference through a mount or link must
//! resolve to the same repository in the destination, except one only
//! back-refs make, which is listed as not copied when it does not. A link
//! inside the set is refused. The plugin's own refusals are decided before anything is
//! written. Without `yes` the call is a preview: it writes nothing and
//! prints the command that writes. With `yes`, a link or back-ref the share
//! cannot copy is refused unless `allow-uncopied` is given. Then one batch
//! put writes the missing schemas and the set through the mount, which makes
//! one commit in the mounted board and moves the mount's pin to it.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{
    ATTACHMENT_TYPE, Fields, Location, LogicalPath, PathError, Refusal, canonical_json, parse_json,
};
use board_ops::{
    Absent, Board, Host, Limit, PluginError, RefsReport, literal, source_name, uri_of,
};

use crate::closure::{Closure, Extra, Mounted, NotCopied, Reason, Whole, closure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// What `board get` answers for a path holding nothing, or only a directory,
/// beside which a record may go.
const ABSENT: [&str; 2] = [NOT_FOUND, "board::record::not_a_record"];
/// What `board refs` answers for a path holding nothing.
const NOT_FOUND: &str = "board::path::not_found";
/// The most paths the output lists (STANDARDS.md 23); `Output` states it.
const LIST_LIMIT: usize = 100;

/// Copy the entry at `path`, its children and every entry its records reference into the board mounted at `to`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ToInput")]
pub(crate) struct Input {
    /// The record or file to share, such as /evals/x; its children under /evals/x/ go with it.
    #[schemars(length(min = 1), extend("format" = "board-path"))]
    path: String,
    /// The mount record of the board to copy into, such as /team; the share writes through it. To share into a team's board, clone its repository, run `board init` there if it is empty, commit and push it, then run `board mount add /team <clone>` here, which follows the branch checked out in the clone. plugins/share/README.md, Setting up a shared board, gives the commands.
    #[schemars(length(min = 1), extend("format" = "board-path"))]
    to: String,
    /// Write the copies, when true; without it the call lists what it would copy and writes nothing.
    #[serde(default)]
    yes: bool,
    /// With yes, write the set even when a link or back-ref names something the share cannot copy, when true.
    #[serde(default, rename = "allow-uncopied")]
    allow_uncopied: bool,
    /// Copy an attachment whose bytes are on this machine, which other readers cannot fetch, when true.
    #[serde(default)]
    local_attachments: bool,
}

/// What the share copied, or would copy without `yes`.
///
/// `shared` is the path shared. `extras` lists, at most 100, the paths in
/// the set outside it, each with `via`, how it was reached (`ref`, `link`
/// or `back_ref`), and `from`, the entry that reached it first;
/// `extras_total` counts them all. `not_copied` lists, at most 100, the
/// link and back-ref targets, and the references through a mount that only
/// back-refs make, that the share leaves behind, each with its `reason`
/// and `from`; `not_copied_total` counts them all. `wholes` lists, at most
/// 100, the wholes outside the shared path whose back-refs the set holds,
/// each with `path`, `from`, the part that names it, and `back_refs`, how
/// many records it added; `wholes_total` counts them all. `next` is the
/// command that writes, null after a write. `repository` is the repository
/// the mount reaches. `commit` is the commit in it that holds the copies,
/// which the mount's pin now names, null when nothing was written. `types`
/// are the types whose schemas were, or would be, added. `copied` lists at
/// most 100 paths; `total` counts them all, and `truncated` says whether
/// some were left out. `uris` gives the board:// URI of each path `copied`
/// lists, in order, by which a reader with a clone of the destination reads
/// it, and is empty when the destination's repository has no origin.
/// `unchanged` counts the paths the destination already held equally.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ToOutput")]
pub(crate) struct Output {
    shared: String,
    extras: Vec<Extra>,
    extras_total: usize,
    not_copied: Vec<NotCopied>,
    not_copied_total: usize,
    wholes: Vec<Whole>,
    wholes_total: usize,
    next: Option<String>,
    repository: String,
    commit: Option<String>,
    types: Vec<String>,
    copied: Vec<String>,
    uris: Vec<String>,
    total: usize,
    truncated: bool,
    unchanged: usize,
}

/// What a batch line writes at a path: a record, or a text file.
#[derive(Debug, PartialEq)]
pub(crate) enum Line {
    Record(Value),
    Text(String),
}

/// Copies `path`'s set into the board mounted at `to` with `yes`, or lists
/// it without.
pub(crate) fn share_to(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        to,
        yes,
        allow_uncopied,
        local_attachments,
    } = PluginError::read_input("share", "to", input)?;
    let (repository, _) = mount(host, &to)?;
    let Closure {
        found,
        mounted,
        extras,
        mut not_copied,
        wholes,
    } = closure(host, &path)?;
    not_copied.extend(unresolved(host, &to, &mounted)?);
    not_copied.sort_by(|one, other| one.path.cmp(&other.path));
    if yes && !allow_uncopied {
        uncopied(&not_copied)?;
    }
    let (lines, types) = read(host, &found, local_attachments)?;
    let targets: Vec<String> = lines.keys().map(|target| format!("{to}{target}")).collect();
    let mut unchanged = BTreeSet::new();
    let mut differs = Vec::new();
    for ((target, line), now) in lines.iter().zip(held(host, &targets)?) {
        match now {
            Some(bytes) if same(line, &bytes) => {
                unchanged.insert(target.clone());
            }
            Some(_) => differs.push(target.clone()),
            None => {}
        }
    }
    let local: Vec<String> = types.iter().map(|name| schema_path(name)).collect();
    let schemas = schemas(host, &types, &local)?;
    let there: Vec<String> = local.iter().map(|schema| format!("{to}{schema}")).collect();
    let mut missing = Vec::new();
    for ((name, schema), now) in schemas.iter().zip(held(host, &there)?) {
        match now {
            None => missing.push(name.clone()),
            Some(bytes) if same(&Line::Record(schema.clone()), &bytes) => {}
            Some(_) => differs.push(schema_path(name)),
        }
    }
    refuse(
        "share::target::differs",
        "the destination already holds something else at",
        &differs,
    )?;
    let copied: Vec<&String> = (lines.keys())
        .filter(|target| !unchanged.contains(*target))
        .collect();
    let mut commit = None;
    if yes && !(missing.is_empty() && copied.is_empty()) {
        let mut batch: Vec<Value> = (missing.iter())
            .map(|name| json!({"path": format!("{to}{}", schema_path(name)), "record": schemas[name]}))
            .collect();
        batch.extend(copied.iter().map(|target| {
            let mut line = json!({"path": format!("{to}{target}"), "expect": "none"});
            match &lines[*target] {
                Line::Record(record) => line["record"] = record.clone(),
                Line::Text(text) => line["text"] = Value::from(text.as_str()),
            }
            line
        }));
        (host.put(&batch)).map_err(|error| {
            PluginError::board_refused("share", "put --batch", &error.refusal())
        })?;
        commit = Some(mount(host, &to)?.1);
    }
    let uncopied = !not_copied.is_empty();
    let next = (!yes)
        .then(|| next(host, &path, &to, local_attachments, uncopied))
        .transpose()?;
    let (extras, extras_total) = capped(extras, LIST_LIMIT);
    let (not_copied, not_copied_total) = capped(not_copied, LIST_LIMIT);
    let (wholes, wholes_total) = capped(wholes, LIST_LIMIT);
    let listed: Vec<String> = (copied.iter().take(LIST_LIMIT))
        .map(|target| (*target).clone())
        .collect();
    let output = Output {
        shared: path,
        extras,
        extras_total,
        not_copied,
        not_copied_total,
        wholes,
        wholes_total,
        next,
        uris: uris(&repository, &listed),
        repository,
        commit,
        types: missing,
        copied: listed,
        total: copied.len(),
        truncated: copied.len() > LIST_LIMIT,
        unchanged: unchanged.len(),
    };
    Ok(serde_json::to_value(output).expect("the output is strings, numbers and booleans"))
}

/// The schema of each of `types`, read from `local`, this board's paths of
/// them.
fn schemas(
    host: &dyn Host,
    types: &[String],
    local: &[String],
) -> Result<BTreeMap<String, Value>, PluginError> {
    let mut schemas = BTreeMap::new();
    for (name, read) in types.iter().zip(get(host, local)?) {
        let bytes = read.map_err(|refusal| PluginError::board_refused("share", "get", &refusal))?;
        // The index refuses every record of a type whose schema does not
        // parse, so this is reached only when the tree changed between the
        // query and this read.
        let Ok(schema) = serde_json::from_slice::<Value>(&bytes) else {
            return Err(refusal(
                "share::record::invalid",
                "this board's schema does not parse as JSON at",
                &[schema_path(name)],
            ));
        };
        schemas.insert(name.clone(), schema);
    }
    Ok(schemas)
}

/// The command that writes what the preview of sharing `path` to `to`
/// lists. It names this board with `-C`, so it writes the same board from
/// any directory.
fn next(
    host: &dyn Host,
    path: &str,
    to: &str,
    local_attachments: bool,
    uncopied: bool,
) -> Result<String, PluginError> {
    let rows = query(
        host,
        "SELECT repository FROM sources WHERE source = 'worktree'",
    )?;
    let board = (rows.first().and_then(|row| row.first()))
        .and_then(Value::as_str)
        .map_or_else(
            || "board".to_owned(),
            |root| format!("board -C {}", quote(root)),
        );
    let mut next = format!(
        "{board} share to --path {} --to {} --yes",
        quote(path),
        quote(to)
    );
    if local_attachments {
        next.push_str(" --local_attachments");
    }
    if uncopied {
        next.push_str(" --allow-uncopied");
    }
    Ok(next)
}

/// The first `cap` of `items`, and how many there were.
pub(crate) fn capped<T>(mut items: Vec<T>, cap: usize) -> (Vec<T>, usize) {
    let total = items.len();
    items.truncate(cap);
    (items, total)
}

/// The URI of each of `paths` in the board whose repository is at
/// `repository`, or none when it has no origin.
fn uris(repository: &str, paths: &[String]) -> Vec<String> {
    let Ok(board) = Board::open(std::path::Path::new(repository), None) else {
        return Vec::new();
    };
    let uri = |path: &String| uri_of(board.repository(), &path.parse().ok()?).ok();
    paths
        .iter()
        .map(uri)
        .collect::<Option<_>>()
        .unwrap_or_default()
}

/// The repository the mount at `to` reaches and the commit its pin names.
///
/// `to` must be the mount record itself: not a path inside a mount, a mount
/// inside a mounted board, or a link to a mount, which a batch cannot write
/// through.
fn mount(host: &dyn Host, to: &str) -> Result<(String, String), PluginError> {
    let not_found = || PluginError {
        code: "share::mount::not_found".to_owned(),
        message: format!(
            "{to} is not the mount record of another repository; `board ls {to}` shows what is \
             there"
        ),
        fields: Map::new(),
        hint: None,
    };
    let Ok(path) = to.parse::<LogicalPath>() else {
        return Err(not_found());
    };
    let answers = refs(host, &[path])?;
    let Some(Some(location)) = answers.into_iter().next() else {
        return Err(not_found());
    };
    match location.pointers.as_slice() {
        [pointer]
            if location.path.is_empty()
                && pointer.board.repository != location.board.repository =>
        {
            Ok((repository(&location), source_name(&location.board.source)))
        }
        _ => Err(not_found()),
    }
}

/// Refuses, naming each with its reason, the link and back-ref targets the
/// share cannot copy.
fn uncopied(not_copied: &[NotCopied]) -> Result<(), PluginError> {
    if not_copied.is_empty() {
        return Ok(());
    }
    let paths: Vec<String> = not_copied.iter().map(|left| left.path.clone()).collect();
    let named: Vec<String> = (not_copied.iter())
        .map(|left| format!("{} ({})", left.path, left.reason.name()))
        .collect();
    let mut refused = refusal(
        "share::closure::uncopied",
        "a link or back-ref names what the share cannot copy:",
        &named,
    );
    refused.fields.insert("paths".to_owned(), json!(paths));
    refused.hint = Some(
        "change the records, or run the call again with --allow-uncopied to share without them"
            .to_owned(),
    );
    Err(refused)
}

/// Refuses each reference through a mount or link that the destination
/// does not resolve into the same repository, and lists as not copied each
/// such reference that only back-refs make.
fn unresolved(
    host: &dyn Host,
    to: &str,
    mounted: &BTreeMap<String, Mounted>,
) -> Result<Vec<NotCopied>, PluginError> {
    let (mut lost, mut left) = (Vec::new(), Vec::new());
    for (target, made) in mounted {
        let resolved = match format!("{to}{target}").parse::<LogicalPath>() {
            Ok(path) => refs(host, &[path])?.into_iter().next().flatten(),
            Err(_) => None,
        };
        if matches!(resolved, Some(location) if repository(&location) == made.repository()) {
            continue;
        }
        match made {
            Mounted::Kept { .. } => lost.push(target.clone()),
            Mounted::BackRef { from, .. } => left.push(NotCopied {
                path: target.clone(),
                reason: Reason::Mount,
                from: from.clone(),
            }),
        }
    }
    refuse(
        "share::closure::unresolved",
        "the destination does not resolve, into the same repository,",
        &lost,
    )?;
    Ok(left)
}

/// What one entry of the set is, by the index.
struct Entry {
    kind: String,
    record_type: Option<String>,
    role: Option<String>,
}

/// What the index says each of `paths` is, refusing a link and an entry
/// that `put` would refuse.
fn classified(host: &dyn Host, paths: &[String]) -> Result<BTreeMap<String, Entry>, PluginError> {
    let listed: Vec<String> = paths.iter().map(|path| literal(path)).collect();
    let rows = query(
        host,
        &format!(
            "SELECT logical, kind, type, code, role FROM entries WHERE source = 'worktree' AND \
             logical IN ({})",
            listed.join(", ")
        ),
    )?;
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let mut entries = BTreeMap::new();
    let (mut pointers, mut invalid) = (Vec::new(), Vec::new());
    for row in rows {
        let [
            Value::String(logical),
            Value::String(kind),
            record_type,
            code,
            role,
        ] = row.as_slice()
        else {
            continue;
        };
        if kind == "pointer" {
            pointers.push(logical.clone());
        }
        if refused(kind, code.as_str()) {
            invalid.push(logical.clone());
        }
        let entry = Entry {
            kind: kind.clone(),
            record_type: text(record_type),
            role: text(role),
        };
        entries.insert(logical.clone(), entry);
    }
    pointers.sort();
    refuse(
        "share::link::unsupported",
        "a batch would write a copy of the target in place of each link",
        &pointers,
    )?;
    invalid.sort();
    refuse(
        "share::record::invalid",
        "`board put` would refuse, as this board's index does,",
        &invalid,
    )?;
    Ok(entries)
}

/// Each path's batch line, a record or a text file, and the types of the
/// records, other than Board's own.
fn read(
    host: &dyn Host,
    paths: &[String],
    local_attachments: bool,
) -> Result<(BTreeMap<String, Line>, Vec<String>), PluginError> {
    let entries = classified(host, paths)?;
    let mut lines = BTreeMap::new();
    let mut types = BTreeSet::new();
    let (mut unsupported, mut local) = (Vec::new(), Vec::new());
    for (path, read) in paths.iter().zip(get(host, paths)?) {
        let bytes = read.map_err(|refusal| PluginError::board_refused("share", "get", &refusal))?;
        let Some(Entry {
            kind,
            record_type,
            role,
        }) = entries.get(path)
        else {
            unsupported.push(path.clone());
            continue;
        };
        let utf8 = String::from_utf8(bytes.clone()).ok();
        let record = serde_json::from_slice::<Value>(&bytes).ok();
        let line = match (kind.as_str(), role.as_deref()) {
            // The file is copied as written, and its frontmatter is checked
            // again by the destination's put.
            ("record", Some("markdown")) => {
                types.extend(record_type.clone());
                utf8.map(Line::Text)
            }
            ("record", _) => record.map(|record| {
                let location = record.get("location").and_then(Value::as_str);
                if record_type.as_deref() == Some(ATTACHMENT_TYPE)
                    && matches!(scheme(location.unwrap_or("None")).as_str(), "" | "file")
                {
                    local.push(path.clone());
                } else if let Some(name) = record_type
                    .as_ref()
                    .filter(|name| !name.starts_with("board/"))
                {
                    types.insert(name.clone());
                }
                Line::Record(record)
            }),
            ("file", _) => utf8.map(Line::Text),
            _ => None,
        };
        match line {
            Some(line) => {
                lines.insert(path.clone(), line);
            }
            None => unsupported.push(path.clone()),
        }
    }
    refuse(
        "share::file::unsupported",
        "a batch writes only records and UTF-8 files, so it cannot copy",
        &unsupported,
    )?;
    if !local_attachments {
        refuse(
            "share::attachment::local",
            "other readers cannot fetch the bytes, kept on this machine, of",
            &local,
        )?;
    }
    Ok((lines, types.into_iter().collect()))
}

/// Whether `put` refuses an entry of `kind` that the index gives `code`.
///
/// A broken reference is admitted, as here. Any other code on a record is a
/// refusal, and so is frontmatter that claims a type and does not parse,
/// whose file is not a record.
pub(crate) fn refused(kind: &str, code: Option<&str>) -> bool {
    let Some(code) = code.filter(|code| !code.is_empty()) else {
        return false;
    };
    code.starts_with("board::frontmatter::")
        || (kind == "record" && !code.starts_with("board::ref::"))
}

/// The scheme of a URL, in lower case, as Python's `urlsplit` reads it: the
/// letters before the first `:` when they form one, or empty.
pub(crate) fn scheme(url: &str) -> String {
    let Some((scheme, _)) = url.split_once(':') else {
        return String::new();
    };
    let mut characters = scheme.chars();
    let starts = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic());
    if starts && characters.all(|rest| rest.is_ascii_alphanumeric() || "+-.".contains(rest)) {
        scheme.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// `text` quoted for a POSIX shell, as Python's `shlex.quote` quotes it.
fn quote(text: &str) -> String {
    let safe =
        |character: char| character.is_ascii_alphanumeric() || "@%+=:,./-_".contains(character);
    if !text.is_empty() && text.chars().all(safe) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

/// The logical path of a type's schema.
pub(crate) fn schema_path(record_type: &str) -> String {
    format!("/.board/types/{record_type}.schema")
}

/// Whether the destination holds `line` already: a record with an equal
/// value, or a file with equal bytes. Values are compared as canonical JSON
/// text, as the Python plugin compared `json.dumps` with sorted keys, so key
/// order does not matter while `true` and `1`, and `-0.0` and `0.0`, differ.
/// A held number Board cannot hold exactly differs from every source value.
pub(crate) fn same(line: &Line, now: &[u8]) -> bool {
    match line {
        Line::Text(text) => now == text.as_bytes(),
        Line::Record(record) => parse_json(now)
            .is_ok_and(|now| canonical_json(&now).as_str() == canonical_json(record).as_str()),
    }
}

/// The bytes at each of `paths`, or none where the destination holds
/// nothing. Board's other refusals are `share::board::refused`.
pub(crate) fn held(host: &dyn Host, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, PluginError> {
    (get(host, paths)?.into_iter())
        .map(|read| match read {
            Ok(bytes) => Ok(Some(bytes)),
            Err(refusal) if ABSENT.contains(&refusal.code.as_str()) => Ok(None),
            Err(refusal) => Err(PluginError::board_refused("share", "get", &refusal)),
        })
        .collect()
}

/// One path's bytes, or Board's refusal of it.
type Read = Result<Vec<u8>, Box<Refusal>>;

/// Reads `paths` as `board get` does, each in order: its bytes, or Board's
/// refusal of it.
fn get(host: &dyn Host, paths: &[String]) -> Result<Vec<Read>, PluginError> {
    let asked: Vec<LogicalPath> = paths
        .iter()
        .map(|path| logical(path, "get"))
        .collect::<Result<_, _>>()?;
    let answers = (host.get_each(&asked))
        .map_err(|error| PluginError::board_refused("share", "get", &error.refusal("get")))?;
    Ok((answers.into_iter())
        .map(|answer| {
            answer
                .map(|report| report.bytes)
                .map_err(|error| Box::new(error.refusal("get")))
        })
        .collect())
}

/// Answers `paths` as `board refs` does: each one's report, or none where
/// nothing is there. Board's other refusals are `share::board::refused`,
/// as the Python plugin's were.
pub(crate) fn reports(
    host: &dyn Host,
    paths: &[LogicalPath],
) -> Result<Vec<Option<RefsReport>>, PluginError> {
    let refused = |refusal: &Refusal| PluginError::board_refused("share", "refs", refusal);
    let answers =
        (host.refs(paths, Absent::Refuse, &[])).map_err(|error| refused(&error.refusal()))?;
    (answers.into_iter())
        .map(|answer| match answer {
            Ok(report) => Ok(Some(report)),
            Err(error) => {
                let refusal = error.refusal();
                if refusal.code.as_str() == NOT_FOUND {
                    Ok(None)
                } else {
                    Err(refused(&refusal))
                }
            }
        })
        .collect()
}

/// Each of `paths`' locations, as [`reports`] answers them.
fn refs(host: &dyn Host, paths: &[LogicalPath]) -> Result<Vec<Option<Location>>, PluginError> {
    let answers = reports(host, paths)?;
    Ok((answers.into_iter())
        .map(|answer| answer.map(|report| report.location))
        .collect())
}

/// The rows `sql` selects, all of them.
pub(crate) fn query(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("share", "query", &error.refusal()))?;
    Ok(report.rows)
}

/// `path` as a logical path, or `share::board::refused` with Board's code
/// when it is not one, as `board <command>` would refuse it.
pub(crate) fn logical(path: &str, command: &str) -> Result<LogicalPath, PluginError> {
    path.parse().map_err(|error: PathError| {
        let refusal = Refusal::new(error.code(), &error, &Fields::default());
        PluginError::board_refused("share", command, &refusal)
    })
}

/// The repository a location is in, as `board refs` prints it.
pub(crate) fn repository(location: &Location) -> String {
    location.board.repository.to_string_lossy().into_owned()
}

/// Refuses with `code`, naming every one of `paths`, when there are any.
fn refuse(code: &str, why: &str, paths: &[String]) -> Result<(), PluginError> {
    if paths.is_empty() {
        Ok(())
    } else {
        Err(refusal(code, why, paths))
    }
}

/// `code`, naming every one of `paths` in its message and its `paths`
/// field.
pub(crate) fn refusal(code: &str, why: &str, paths: &[String]) -> PluginError {
    let mut named = paths
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > 10 {
        named = format!("{named} and {} more", paths.len() - 10);
    }
    let mut fields = Map::new();
    fields.insert("paths".to_owned(), json!(paths));
    PluginError {
        code: code.to_owned(),
        message: format!("{why} {named}; nothing was written"),
        fields,
        hint: None,
    }
}
