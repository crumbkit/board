//! `closure`: the set a share copies.
//!
//! The set is the entry at the shared path, the entries under it, and every
//! entry in this board's own tree that is reached from them, followed until
//! nothing new is reached:
//!
//! - a typed reference a record's schema declares, including one in a
//!   Markdown file's frontmatter, reaches its target, and one to a directory
//!   reaches the entries under it;
//! - a Markdown link reaches the record or file it names;
//! - a record whose typed reference names an entry that collects back-refs
//!   is a back-ref, and is reached.
//!
//! The shared entries collect back-refs, and so do wholes. A whole is a
//! record that an entry collecting back-refs names through a part-of field,
//! one whose schema in this board's `.board/types/` carries
//! `"x-board-part-of": true`, such as a task's `parent`. Wholes are found
//! upward until no new one appears. A whole's back-ref is skipped when each
//! typed reference it makes to the whole is a part-of field: it is a
//! sibling part. Back-refs and the records a reference or link reaches
//! collect none.
//!
//! A typed reference through a mount or link is not copied; the caller
//! checks that the destination resolves it, and lists one a back-ref makes
//! as not copied. A link or back-ref share cannot copy is listed with the
//! reason.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{Location, LogicalPath, Object, Source};
use board_ops::{Host, Outgoing, PluginError, literal};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::to::{held, logical, query, refusal, reports, repository, schema_path};

/// How an extra path was reached: a typed reference, a Markdown link, or
/// a record whose typed reference names an entry that collects back-refs.
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Via {
    Ref,
    Link,
    BackRef,
}

/// A path in the set outside the shared path, how it was reached, and the
/// entry that reached it first.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Extra {
    path: String,
    via: Via,
    from: String,
}

/// A link or back-ref target the share does not copy, why, and the entry
/// that reached it first.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NotCopied {
    pub(crate) path: String,
    pub(crate) reason: Reason,
    pub(crate) from: String,
}

/// A whole outside the shared path, the part that names it, and how many
/// records its back-refs added to the set.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Whole {
    path: String,
    from: String,
    back_refs: usize,
}

/// Why a target is not copied: it is a directory, whose entries a link does
/// not reach; it resolves through a mount or a link; or it names nothing.
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Reason {
    Directory,
    Mount,
    Link,
    Missing,
}

impl Reason {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Mount => "mount",
            Self::Link => "link",
            Self::Missing => "missing",
        }
    }
}

/// A typed reference through a mount or link, the repository it reaches,
/// and whether a back-ref made it.
#[derive(Clone)]
pub(crate) enum Mounted {
    /// An entry that is not a back-ref made it, so the destination must
    /// resolve it.
    Kept { repository: String },
    /// Only back-refs made it, the first `from`; the share lists it as not
    /// copied when the destination does not resolve it.
    BackRef { repository: String, from: String },
}

impl Mounted {
    pub(crate) fn repository(&self) -> &str {
        match self {
            Self::Kept { repository } | Self::BackRef { repository, .. } => repository,
        }
    }
}

/// What [`closure`] finds.
pub(crate) struct Closure {
    /// The paths to copy, sorted.
    pub(crate) found: Vec<String>,
    /// Each typed reference through a mount or link.
    pub(crate) mounted: BTreeMap<String, Mounted>,
    /// The paths outside the shared path, sorted.
    pub(crate) extras: Vec<Extra>,
    /// The targets not copied, sorted.
    pub(crate) not_copied: Vec<NotCopied>,
    /// The wholes outside the shared path, sorted.
    pub(crate) wholes: Vec<Whole>,
}

/// A path reached, how, and from which entry.
type Reached = BTreeMap<String, (Via, String)>;

/// Each candidate back-ref, with each entry it may name and that entry's
/// location.
type Candidates = BTreeMap<String, Vec<(String, Location)>>;

/// An entry in this board's own tree: its kind and its record type.
#[derive(Clone)]
struct Held {
    kind: String,
    record_type: Option<String>,
}

/// The set sharing `path` copies.
pub(crate) fn closure(host: &dyn Host, path: &str) -> Result<Closure, PluginError> {
    let mut found = entries(host, &under(&[path.to_owned()]))?;
    if found.is_empty() {
        return Err(PluginError {
            code: "share::path::not_found".to_owned(),
            message: format!("{path} names nothing in this board's own tree"),
            fields: Map::new(),
            hint: None,
        });
    }
    let inside = |target: &str| target == path || target.starts_with(&format!("{path}/"));
    let mut walk = Walk::default();
    let mut extras = Reached::new();
    let mut frontier = found.clone();
    // The entries that collect back-refs this round: the shared entries
    // first, then each round's new wholes.
    let mut collect: BTreeSet<String> = found.keys().cloned().collect();
    let mut collected = BTreeSet::new();
    while !frontier.is_empty() || !collect.is_empty() {
        let asked: Vec<String> = (frontier.iter())
            .filter(|(_, held)| held.kind == "record" || held.kind == "file")
            .map(|(target, _)| target.clone())
            .chain(
                collect
                    .iter()
                    .filter(|whole| !frontier.contains_key(*whole))
                    .cloned(),
            )
            .collect();
        let (mut reached, mut candidates, next) =
            walk.ask(host, &asked, &found, (&collect, &collected), &inside)?;
        collected.append(&mut collect);
        collect = next;
        candidates.retain(|candidate, _| !found.contains_key(candidate));
        walk.back_refs(host, candidates, &mut reached)?;
        reached.retain(|target, _| !found.contains_key(target));
        if reached.is_empty() {
            frontier.clear();
            continue;
        }
        let listed: Vec<String> = reached.keys().map(|target| literal(target)).collect();
        let mut held = entries(host, &format!("logical IN ({})", listed.join(", ")))?;
        // A typed reference to a directory has no entry of its own, and
        // reaches the entries under it.
        let directories: Vec<String> = (reached.keys())
            .filter(|target| !held.contains_key(*target))
            .cloned()
            .collect();
        held.extend(entries(host, &under(&directories))?);
        held.retain(|target, _| !found.contains_key(target));
        walk.count(held.keys(), &reached);
        for target in held.keys().filter(|target| !inside(target)) {
            let origin = (directories.iter())
                .find(|directory| target.starts_with(&format!("{directory}/")))
                .unwrap_or(target);
            if let Some(how) = reached.get(origin) {
                extras.entry(target.clone()).or_insert_with(|| how.clone());
            }
        }
        walk.expanded.extend(directories);
        frontier = held.clone();
        found.extend(held);
    }
    Ok(walk.finish(found.into_keys().collect(), extras, &inside))
}

/// What a walk has met besides the set.
#[derive(Default)]
struct Walk {
    mounted: BTreeMap<String, Mounted>,
    left: BTreeMap<String, (Reason, String)>,
    expanded: BTreeSet<String>,
    /// Each whole outside the shared path, with the part that named it and
    /// how many back-refs it added.
    wholes: BTreeMap<String, (String, usize)>,
    /// The entries in the set that were reached as back-refs.
    back: BTreeSet<String>,
    /// Each record type's schema in this board, or none when it has none.
    schemas: BTreeMap<String, Option<Value>>,
}

impl Walk {
    /// Asks for the references from and to each of `asked`. Each of
    /// `collect`, the first of `collecting`, gives its candidate back-refs
    /// and names its new wholes, which are neither inside the shared path
    /// nor among `collecting`.
    fn ask(
        &mut self,
        host: &dyn Host,
        asked: &[String],
        found: &BTreeMap<String, Held>,
        collecting: (&BTreeSet<String>, &BTreeSet<String>),
        inside: &dyn Fn(&str) -> bool,
    ) -> Result<(Reached, Candidates, BTreeSet<String>), PluginError> {
        let (collect, collected) = collecting;
        let logicals: Vec<LogicalPath> = (asked.iter())
            .map(|target| logical(target, "refs"))
            .collect::<Result<_, _>>()?;
        let mut reached = Reached::new();
        let mut candidates = Candidates::new();
        let mut next = BTreeSet::new();
        for (entry, report) in asked.iter().zip(reports(host, &logicals)?) {
            let Some(report) = report else { continue };
            let collects = collect.contains(entry);
            let record_type = found.get(entry).and_then(|held| held.record_type.clone());
            for reference in report.from {
                if collects
                    && let Some(whole) = self.whole(host, record_type.as_deref(), &reference)?
                    && !inside(&whole)
                    && !collected.contains(&whole)
                    && !collect.contains(&whole)
                {
                    self.back.remove(&whole);
                    (self.wholes.entry(whole.clone())).or_insert_with(|| (entry.clone(), 0));
                    next.insert(whole);
                }
                self.follow(entry, reference, &mut reached);
            }
            for incoming in report.to.into_iter().filter(|_| collects) {
                if let (Source::WorkingTree, Some(candidate)) = (incoming.source, incoming.logical)
                {
                    (candidates.entry(candidate.to_string()).or_default())
                        .push((entry.clone(), report.location.clone()));
                }
            }
        }
        Ok((reached, candidates, next))
    }

    /// Notes each of `added` that `reached` reached as a back-ref, and
    /// counts it for the whole that brought it.
    fn count<'a>(&mut self, added: impl Iterator<Item = &'a String>, reached: &Reached) {
        for target in added {
            if let Some((Via::BackRef, from)) = reached.get(target) {
                self.back.insert(target.clone());
                if let Some((_, count)) = self.wholes.get_mut(from) {
                    *count += 1;
                }
            }
        }
    }

    /// The closure of `found`, with `extras`, once the walk has ended.
    fn finish(self, found: Vec<String>, extras: Reached, inside: &dyn Fn(&str) -> bool) -> Closure {
        let Self {
            mounted,
            left,
            expanded,
            wholes,
            ..
        } = self;
        // A directory under the shared path, or at or under one a typed
        // reference reached, has its entries copied anyway.
        let copied = |target: &str| {
            inside(target)
                || (expanded.iter()).any(|directory| {
                    target == directory || target.starts_with(&format!("{directory}/"))
                })
        };
        let not_copied = (left.into_iter())
            .filter(|(target, (reason, _))| {
                !(matches!(reason, Reason::Directory) && copied(target))
            })
            .map(|(path, (reason, from))| NotCopied { path, reason, from })
            .collect();
        let extras = (extras.into_iter())
            .map(|(path, (via, from))| Extra { path, via, from })
            .collect();
        let wholes = (wholes.into_iter())
            .map(|(path, (from, back_refs))| Whole {
                path,
                from,
                back_refs,
            })
            .collect();
        Closure {
            found,
            mounted,
            extras,
            not_copied,
            wholes,
        }
    }

    /// Follows one reference that `entry` makes.
    fn follow(&mut self, entry: &str, reference: Outgoing, reached: &mut Reached) {
        let via = if reference.link { Via::Link } else { Via::Ref };
        match (via, reference.location) {
            // A broken typed reference is copied as written.
            (Via::Ref, None) => {}
            (Via::Ref, Some(location)) if outside(&location) => {
                let repository = repository(&location);
                let made = if self.back.contains(entry) {
                    Mounted::BackRef {
                        repository,
                        from: entry.to_owned(),
                    }
                } else {
                    Mounted::Kept { repository }
                };
                let target = reference.target.to_string();
                // A reference an entry other than a back-ref makes is kept
                // even when a back-ref made it first.
                let replace = match self.mounted.get(&target) {
                    None => true,
                    Some(Mounted::BackRef { .. }) => matches!(made, Mounted::Kept { .. }),
                    Some(Mounted::Kept { .. }) => false,
                };
                if replace {
                    self.mounted.insert(target, made);
                }
            }
            (_, None) => self.leave(reference.target.to_string(), Reason::Missing, entry),
            (_, Some(location)) => match (via, reason(&location)) {
                (Via::Link, Some(reason)) => {
                    self.leave(reference.target.to_string(), reason, entry);
                }
                _ => {
                    if let Some(display) = location.display_path() {
                        (reached.entry(display.to_string()))
                            .or_insert_with(|| (via, entry.to_owned()));
                    }
                }
            },
        }
    }

    /// The record `reference` names, when a record of `record_type` makes
    /// it through a part-of field and it lies in this board's own tree.
    fn whole(
        &mut self,
        host: &dyn Host,
        record_type: Option<&str>,
        reference: &Outgoing,
    ) -> Result<Option<String>, PluginError> {
        let Some(location) = reference.location.as_ref().filter(|_| !reference.link) else {
            return Ok(None);
        };
        if reason(location).is_some() || outside(location) {
            return Ok(None);
        }
        if !self.part_of(host, record_type, &reference.pointer)? {
            return Ok(None);
        }
        Ok(location.display_path().map(|display| display.to_string()))
    }

    /// Whether `pointer`, in a record of `record_type`, is a part-of field
    /// by this board's schema of the type.
    fn part_of(
        &mut self,
        host: &dyn Host,
        record_type: Option<&str>,
        pointer: &str,
    ) -> Result<bool, PluginError> {
        let Some(record_type) = record_type else {
            return Ok(false);
        };
        if !self.schemas.contains_key(record_type) {
            let local = schema_path(record_type);
            let read = held(host, std::slice::from_ref(&local))?.pop().flatten();
            let schema = match read {
                None => None,
                Some(bytes) => Some(serde_json::from_slice::<Value>(&bytes).map_err(|_| {
                    refusal(
                        "share::record::invalid",
                        "this board's schema does not parse as JSON at",
                        &[local],
                    )
                })?),
            };
            self.schemas.insert(record_type.to_owned(), schema);
        }
        Ok((self.schemas[record_type].as_ref()).is_some_and(|schema| part_of(schema, pointer)))
    }

    /// Adds to `reached` each of `candidates` whose typed reference names
    /// one of the entries it lists, with its location. A whole's candidate
    /// is skipped when every such reference is a part-of field.
    fn back_refs(
        &mut self,
        host: &dyn Host,
        candidates: Candidates,
        reached: &mut Reached,
    ) -> Result<(), PluginError> {
        if candidates.is_empty() {
            return Ok(());
        }
        let (paths, named): (Vec<String>, Vec<_>) = candidates.into_iter().unzip();
        let listed: Vec<String> = paths.iter().map(|path| literal(path)).collect();
        let types = entries(host, &format!("logical IN ({})", listed.join(", ")))?;
        let logicals: Vec<LogicalPath> = (paths.iter())
            .map(|target| logical(target, "refs"))
            .collect::<Result<_, _>>()?;
        let answers = reports(host, &logicals)?;
        for ((candidate, named), report) in paths.into_iter().zip(named).zip(answers) {
            let Some(report) = report else { continue };
            let record_type = types
                .get(&candidate)
                .and_then(|held| held.record_type.as_deref());
            let mut by = None;
            for (entry, location) in &named {
                let sibling = self.wholes.contains_key(entry);
                for reference in &report.from {
                    let to_entry = !reference.link
                        && (reference.location.as_ref()).is_some_and(|reached| {
                            reached.board == location.board && reached.path == location.path
                        });
                    if to_entry
                        && !(sibling && self.part_of(host, record_type, &reference.pointer)?)
                    {
                        by = Some(entry.clone());
                        break;
                    }
                }
                if by.is_some() {
                    break;
                }
            }
            let Some(entry) = by else { continue };
            match reason(&report.location) {
                Some(reason) => self.leave(candidate, reason, &entry),
                None => {
                    (reached.entry(candidate)).or_insert_with(|| (Via::BackRef, entry));
                }
            }
        }
        Ok(())
    }

    fn leave(&mut self, target: String, reason: Reason, entry: &str) {
        (self.left.entry(target)).or_insert_with(|| (reason, entry.to_owned()));
    }
}

/// The most local `$ref`s one step of [`part_of`] follows, which ends a
/// `$ref` that names itself.
const REF_DEPTH: usize = 16;

/// Whether the field at the JSON `pointer` in a record that `schema`
/// describes carries `"x-board-part-of": true`. The pointer is resolved
/// through `properties`, `items` for a numbered token and local `$ref`s; a
/// pointer that reaches no field schema is not part-of.
pub(crate) fn part_of(schema: &Value, pointer: &str) -> bool {
    let Some(tokens) = pointer.strip_prefix('/') else {
        return false;
    };
    let mut at = schema;
    for token in tokens.split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        let Some(resolved) = dereferenced(schema, at).last().copied() else {
            return false;
        };
        let property = resolved
            .get("properties")
            .and_then(|named| named.get(&token));
        let item = (resolved.get("items"))
            .filter(|_| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()));
        match property.or(item) {
            Some(next) => at = next,
            None => return false,
        }
    }
    (dereferenced(schema, at).iter())
        .any(|step| step.get("x-board-part-of") == Some(&Value::Bool(true)))
}

/// `at`, then each schema its local `$ref` chain names in `root`, or
/// nothing when the chain leaves `root`, dangles or passes [`REF_DEPTH`].
fn dereferenced<'a>(root: &'a Value, at: &'a Value) -> Vec<&'a Value> {
    let mut chain = vec![at];
    let mut step = at;
    while let Some(target) = step.get("$ref").and_then(Value::as_str) {
        let next = target
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer));
        match next {
            Some(next) if chain.len() <= REF_DEPTH => {
                chain.push(next);
                step = next;
            }
            _ => return Vec::new(),
        }
    }
    chain
}

/// Whether `location` is reached through a mount or link, or lies in
/// another board.
fn outside(location: &Location) -> bool {
    !location.pointers.is_empty() || location.board.source != Source::WorkingTree
}

/// Why a link or back-ref to `location` cannot be copied, or none when it
/// can: a pointer to another repository is a mount, one inside this board a
/// link.
fn reason(location: &Location) -> Option<Reason> {
    match location.pointers.first() {
        Some(pointer) if pointer.board.repository == location.board.repository => {
            Some(Reason::Link)
        }
        Some(_) => Some(Reason::Mount),
        None if location.board.source != Source::WorkingTree => Some(Reason::Mount),
        None if location.object == Object::Directory => Some(Reason::Directory),
        None => None,
    }
}

/// An `entries` condition matching each of `paths` and every entry under
/// it, and nothing when there are none.
fn under(paths: &[String]) -> String {
    if paths.is_empty() {
        return "0".to_owned();
    }
    let conditions: Vec<String> = (paths.iter())
        .map(|path| {
            let (path, prefix) = (literal(path), literal(&format!("{path}/")));
            format!("logical = {path} OR substr(logical, 1, length({prefix})) = {prefix}")
        })
        .collect();
    conditions.join(" OR ")
}

/// The logical path, kind and record type of each entry in this board's
/// own tree that matches `condition`.
fn entries(host: &dyn Host, condition: &str) -> Result<BTreeMap<String, Held>, PluginError> {
    let sql = format!(
        "SELECT logical, kind, type FROM entries WHERE source = 'worktree' AND ({condition})"
    );
    let rows = query(host, &sql)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row.as_slice() {
            [Value::String(logical), Value::String(kind), record_type] => {
                let held = Held {
                    kind: kind.clone(),
                    record_type: record_type.as_str().map(str::to_owned),
                };
                Some((logical.clone(), held))
            }
            _ => None,
        })
        .collect())
}
