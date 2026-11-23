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
//! - a record whose typed reference names a shared entry is a back-ref, and
//!   is reached. Only the shared entries collect back-refs. A back-ref
//!   whose type's schema carries `"x-board-goes-with-share": false`, such
//!   as the owner's priorities, is listed as not copied instead.
//!
//! A whole is a record that a shared entry, or another whole, names through
//! a part-of field, one whose schema in this board's `.board/types/`
//! carries `"x-board-part-of": true`, such as a task's `parent`. Wholes are
//! found upward before the walk, and the walk reaches each by its
//! reference. A whole brings, as back-refs, the records that name it by a
//! typed reference and whose type's schema carries
//! `"x-board-goes-with-whole": true`, such as a project's decisions and
//! questions. Those are leaves: they are copied, none of their references
//! or links is followed, and each one to a path the set does not hold is
//! listed as not copied, except a typed reference through a mount or link,
//! which the caller lists only when the destination does not resolve it.
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
/// a record whose typed reference names a shared entry or a whole.
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

/// A target the share does not copy, why, and the entry that reached it
/// first.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NotCopied {
    pub(crate) path: String,
    pub(crate) reason: Reason,
    pub(crate) from: String,
}

impl NotCopied {
    /// Whether a share refuses it without `allow_uncopied`. A withheld
    /// back-ref is listed only.
    pub(crate) fn refused(&self) -> bool {
        !matches!(self.reason, Reason::Withheld)
    }
}

/// A whole outside the shared path, the part that names it, and how many
/// records that go with it it added to the set.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Whole {
    path: String,
    from: String,
    back_refs: usize,
}

/// Why a target is not copied: it is a directory, whose entries a link does
/// not reach; it resolves through a mount or a link; it names nothing; a
/// whole's back-ref names it, whose references the share does not follow;
/// or it is a back-ref whose type keeps it out of shares.
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Reason {
    Directory,
    Mount,
    Link,
    Missing,
    Beyond,
    Withheld,
}

impl Reason {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::Mount => "mount",
            Self::Link => "link",
            Self::Missing => "missing",
            Self::Beyond => "beyond",
            Self::Withheld => "withheld",
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
    /// copied, with `reason`, `link` or `mount`, when the destination does
    /// not resolve it.
    BackRef {
        repository: String,
        from: String,
        reason: Reason,
    },
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

/// An entry in this board's own tree: its kind and its record type.
#[derive(Clone)]
struct Held {
    kind: String,
    record_type: Option<String>,
}

/// A record that goes with a whole: its entry, the whole, and the
/// references it makes, which the share does not follow.
struct Leaf {
    path: String,
    held: Held,
    whole: String,
    from: Vec<Outgoing>,
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
    let leaves = walk.wholes(host, &found, &inside)?;
    let mut extras = Reached::new();
    let mut frontier = found.clone();
    let mut shared = true;
    while !frontier.is_empty() {
        let asked: Vec<String> = (frontier.iter())
            .filter(|(_, held)| held.kind == "record" || held.kind == "file")
            .map(|(target, _)| target.clone())
            .collect();
        let logicals: Vec<LogicalPath> = (asked.iter())
            .map(|target| logical(target, "refs"))
            .collect::<Result<_, _>>()?;
        let mut reached = Reached::new();
        let mut candidates: BTreeMap<String, Vec<(String, Location)>> = BTreeMap::new();
        for (entry, report) in asked.iter().zip(reports(host, &logicals)?) {
            let Some(report) = report else { continue };
            for reference in report.from {
                walk.follow(entry, reference, &mut reached);
            }
            // Only the shared entries collect back-refs.
            for incoming in report.to.into_iter().filter(|_| shared) {
                if let (Source::WorkingTree, Some(candidate)) = (incoming.source, incoming.logical)
                {
                    (candidates.entry(candidate.to_string()).or_default())
                        .push((entry.clone(), report.location.clone()));
                }
            }
        }
        candidates.retain(|candidate, _| !found.contains_key(candidate));
        walk.back_refs(host, candidates, &mut reached)?;
        shared = false;
        reached.retain(|target, _| !found.contains_key(target));
        if reached.is_empty() {
            break;
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
        for target in held.keys() {
            // A whole is kept whole even when a shared entry's back-ref
            // reached it.
            if let Some((Via::BackRef, _)) = reached.get(target)
                && !walk.wholes.contains_key(target)
            {
                walk.back.insert(target.clone());
            }
        }
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
    walk.add(leaves, &mut found, &mut extras, &inside);
    Ok(walk.finish(found.into_keys().collect(), extras, &inside))
}

/// What a walk has met besides the set.
#[derive(Default)]
struct Walk {
    mounted: BTreeMap<String, Mounted>,
    left: BTreeMap<String, (Reason, String)>,
    expanded: BTreeSet<String>,
    /// Each whole outside the shared path, with the part that named it and
    /// how many records that go with it it added.
    wholes: BTreeMap<String, (String, usize)>,
    /// The entries in the set that were reached as back-refs of the shared
    /// entries.
    back: BTreeSet<String>,
    /// Each record type's schema in this board, or none when it has none.
    schemas: BTreeMap<String, Option<Value>>,
}

impl Walk {
    /// Finds the wholes above the records in `shared`, upward along
    /// part-of fields, and the records that go with each of them.
    fn wholes(
        &mut self,
        host: &dyn Host,
        shared: &BTreeMap<String, Held>,
        inside: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<Leaf>, PluginError> {
        let mut frontier: Vec<(String, Option<String>)> = (shared.iter())
            .filter(|(_, held)| held.record_type.is_some())
            .map(|(path, held)| (path.clone(), held.record_type.clone()))
            .collect();
        let mut leaves = Vec::new();
        while !frontier.is_empty() {
            let logicals: Vec<LogicalPath> = (frontier.iter())
                .map(|(target, _)| logical(target, "refs"))
                .collect::<Result<_, _>>()?;
            let mut next = Vec::new();
            let mut candidates: BTreeMap<String, (String, Location)> = BTreeMap::new();
            for ((entry, record_type), report) in frontier.iter().zip(reports(host, &logicals)?) {
                let Some(report) = report else { continue };
                for reference in &report.from {
                    if let Some(whole) = self.whole(host, record_type.as_deref(), reference)?
                        && !inside(&whole)
                        && !self.wholes.contains_key(&whole)
                    {
                        self.wholes.insert(whole.clone(), (entry.clone(), 0));
                        next.push(whole);
                    }
                }
                if !self.wholes.contains_key(entry) {
                    continue;
                }
                for incoming in report.to {
                    if let (Source::WorkingTree, Some(candidate)) =
                        (incoming.source, incoming.logical)
                    {
                        (candidates.entry(candidate.to_string()))
                            .or_insert_with(|| (entry.clone(), report.location.clone()));
                    }
                }
            }
            candidates
                .retain(|candidate, _| !inside(candidate) && !self.wholes.contains_key(candidate));
            leaves.extend(self.goes_with(host, candidates)?);
            let types = typed(host, &next)?;
            frontier = (next.into_iter())
                .map(|whole| {
                    let record_type = types.get(&whole).and_then(|held| held.record_type.clone());
                    (whole, record_type)
                })
                .collect();
        }
        Ok(leaves)
    }

    /// Each of `candidates` whose type's schema carries
    /// `x-board-goes-with-whole` and whose typed reference names the whole
    /// it is listed with.
    fn goes_with(
        &mut self,
        host: &dyn Host,
        candidates: BTreeMap<String, (String, Location)>,
    ) -> Result<Vec<Leaf>, PluginError> {
        let mut types = typed(host, &candidates.keys().cloned().collect::<Vec<_>>())?;
        let mut kept = Vec::new();
        for (candidate, named) in candidates {
            let Some(held) = types.remove(&candidate) else {
                continue;
            };
            let goes = match held.record_type.as_deref() {
                Some(record_type) => (self.schema(host, record_type)?)
                    .is_some_and(|schema| schema["x-board-goes-with-whole"] == true),
                None => false,
            };
            if goes {
                kept.push((candidate, held, named));
            }
        }
        let logicals: Vec<LogicalPath> = (kept.iter())
            .map(|(target, _, _)| logical(target, "refs"))
            .collect::<Result<_, _>>()?;
        let mut leaves = Vec::new();
        for ((path, held, (whole, location)), report) in
            kept.into_iter().zip(reports(host, &logicals)?)
        {
            let Some(report) = report else { continue };
            let names = (report.from.iter()).any(|reference| {
                !reference.link
                    && (reference.location.as_ref()).is_some_and(|reached| {
                        reached.board == location.board && reached.path == location.path
                    })
            });
            if names && reason(&report.location).is_none() {
                leaves.push(Leaf {
                    path,
                    held,
                    whole,
                    from: report.from,
                });
            }
        }
        Ok(leaves)
    }

    /// Adds each of `leaves` the walk did not reach to `found`, counts it
    /// for its whole, and lists each reference it makes to a path the set
    /// does not hold.
    fn add(
        &mut self,
        leaves: Vec<Leaf>,
        found: &mut BTreeMap<String, Held>,
        extras: &mut Reached,
        inside: &dyn Fn(&str) -> bool,
    ) {
        let mut added = Vec::new();
        for leaf in leaves {
            if found.contains_key(&leaf.path) {
                continue;
            }
            found.insert(leaf.path.clone(), leaf.held.clone());
            if !inside(&leaf.path) {
                (extras.entry(leaf.path.clone()))
                    .or_insert_with(|| (Via::BackRef, leaf.whole.clone()));
            }
            if let Some((_, count)) = self.wholes.get_mut(&leaf.whole) {
                *count += 1;
            }
            added.push(leaf);
        }
        for leaf in added {
            for reference in leaf.from {
                let target = reference.target.to_string();
                let why = match reference.location {
                    // The destination may resolve it, which `share.to`
                    // decides.
                    Some(location) if !reference.link && outside(&location) => {
                        let made = Mounted::BackRef {
                            repository: repository(&location),
                            from: leaf.path.clone(),
                            reason: reason(&location).unwrap_or(Reason::Mount),
                        };
                        self.mount(target, made);
                        continue;
                    }
                    None => Some(Reason::Missing),
                    Some(location) => match reason(&location) {
                        Some(Reason::Directory) => Some(Reason::Beyond),
                        Some(reason) => Some(reason),
                        None => (location.display_path())
                            .filter(|display| !found.contains_key(display.as_str()))
                            .map(|_| Reason::Beyond),
                    },
                };
                if let Some(why) = why {
                    self.leave(target, why, &leaf.path);
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
                !(matches!(reason, Reason::Directory | Reason::Beyond) && copied(target))
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
                        reason: reason(&location).unwrap_or(Reason::Mount),
                    }
                } else {
                    Mounted::Kept { repository }
                };
                self.mount(reference.target.to_string(), made);
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
        let Some(record_type) = record_type.filter(|_| reason(location).is_none()) else {
            return Ok(None);
        };
        let marked = (self.schema(host, record_type)?)
            .is_some_and(|schema| part_of(schema, &reference.pointer));
        Ok(marked
            .then(|| location.display_path().map(|display| display.to_string()))
            .flatten())
    }

    /// This board's schema of `record_type`, read once, or none when the
    /// board holds none.
    fn schema(
        &mut self,
        host: &dyn Host,
        record_type: &str,
    ) -> Result<Option<&Value>, PluginError> {
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
        Ok(self.schemas[record_type].as_ref())
    }

    /// Adds to `reached` each of `candidates` whose typed reference names
    /// one of the shared entries it lists, with its location.
    fn back_refs(
        &mut self,
        host: &dyn Host,
        candidates: BTreeMap<String, Vec<(String, Location)>>,
        reached: &mut Reached,
    ) -> Result<(), PluginError> {
        let kinds = typed(host, &candidates.keys().cloned().collect::<Vec<_>>())?;
        let (paths, named): (Vec<String>, Vec<_>) = candidates.into_iter().unzip();
        let logicals: Vec<LogicalPath> = (paths.iter())
            .map(|target| logical(target, "refs"))
            .collect::<Result<_, _>>()?;
        let answers = reports(host, &logicals)?;
        for ((candidate, named), report) in paths.into_iter().zip(named).zip(answers) {
            let Some(report) = report else { continue };
            let typed = |location: &Location| {
                (report.from.iter()).any(|reference| {
                    !reference.link
                        && (reference.location.as_ref()).is_some_and(|reached| {
                            reached.board == location.board && reached.path == location.path
                        })
                })
            };
            let Some((entry, _)) = named.iter().find(|(_, location)| typed(location)) else {
                continue;
            };
            let record_type = (kinds.get(&candidate)).and_then(|held| held.record_type.clone());
            let withheld = match record_type {
                Some(record_type) => (self.schema(host, &record_type)?)
                    .is_some_and(|schema| schema["x-board-goes-with-share"] == false),
                None => false,
            };
            match reason(&report.location) {
                Some(reason) => self.leave(candidate, reason, entry),
                None if withheld => self.leave(candidate, Reason::Withheld, entry),
                None => {
                    (reached.entry(candidate)).or_insert_with(|| (Via::BackRef, entry.clone()));
                }
            }
        }
        Ok(())
    }

    /// Records a typed reference through a mount or link. One an entry
    /// other than a back-ref makes is kept even when a back-ref made it
    /// first.
    fn mount(&mut self, target: String, made: Mounted) {
        let replace = match self.mounted.get(&target) {
            None => true,
            Some(Mounted::BackRef { .. }) => matches!(made, Mounted::Kept { .. }),
            Some(Mounted::Kept { .. }) => false,
        };
        if replace {
            self.mounted.insert(target, made);
        }
    }

    fn leave(&mut self, target: String, reason: Reason, entry: &str) {
        (self.left.entry(target)).or_insert_with(|| (reason, entry.to_owned()));
    }
}

/// The entry and type of each of `paths` in this board's own tree.
fn typed(host: &dyn Host, paths: &[String]) -> Result<BTreeMap<String, Held>, PluginError> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let listed: Vec<String> = paths.iter().map(|path| literal(path)).collect();
    entries(host, &format!("logical IN ({})", listed.join(", ")))
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
