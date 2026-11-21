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
//! - a record whose typed reference names the shared entry or an entry under
//!   it is a back-ref, and is reached. Only the shared entries collect
//!   back-refs.
//!
//! A typed reference through a mount or link is not copied; the caller
//! checks that the destination resolves it. A link or back-ref share cannot
//! copy is listed with the reason.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{Location, LogicalPath, Object, Source};
use board_ops::{Host, Outgoing, PluginError, literal};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::to::{logical, query, reports, repository};

/// How an extra path was reached: a typed reference, a Markdown link, or
/// a record whose typed reference names a shared entry.
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
    from: String,
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

/// What [`closure`] finds.
pub(crate) struct Closure {
    /// The paths to copy, sorted.
    pub(crate) found: Vec<String>,
    /// Each typed reference through a mount or link, with the repository it
    /// reaches.
    pub(crate) mounted: BTreeMap<String, String>,
    /// The paths outside the shared path, sorted.
    pub(crate) extras: Vec<Extra>,
    /// The targets not copied, sorted.
    pub(crate) not_copied: Vec<NotCopied>,
}

/// A path reached, how, and from which entry.
type Reached = BTreeMap<String, (Via, String)>;

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
    let mut shared = true;
    while !frontier.is_empty() {
        let asked: Vec<String> = (frontier.iter())
            .filter(|(_, kind)| *kind == "record" || *kind == "file")
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
    let Walk {
        mounted,
        left,
        expanded,
    } = walk;
    // A directory under the shared path, or one a typed reference reached,
    // has its entries copied anyway.
    let not_copied = (left.into_iter())
        .filter(|(target, (reason, _))| {
            !(matches!(reason, Reason::Directory) && (inside(target) || expanded.contains(target)))
        })
        .map(|(path, (reason, from))| NotCopied { path, reason, from })
        .collect();
    let extras = (extras.into_iter())
        .map(|(path, (via, from))| Extra { path, via, from })
        .collect();
    Ok(Closure {
        found: found.into_keys().collect(),
        mounted,
        extras,
        not_copied,
    })
}

/// What a walk has met besides the set.
#[derive(Default)]
struct Walk {
    mounted: BTreeMap<String, String>,
    left: BTreeMap<String, (Reason, String)>,
    expanded: BTreeSet<String>,
}

impl Walk {
    /// Follows one reference that `entry` makes.
    fn follow(&mut self, entry: &str, reference: Outgoing, reached: &mut Reached) {
        let via = if reference.link { Via::Link } else { Via::Ref };
        match (via, reference.location) {
            // A broken typed reference is copied as written.
            (Via::Ref, None) => {}
            (Via::Ref, Some(location)) if outside(&location) => {
                (self.mounted).insert(reference.target.to_string(), repository(&location));
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

    /// Adds to `reached` each of `candidates` whose typed reference names
    /// one of the shared entries it lists, with its location.
    fn back_refs(
        &mut self,
        host: &dyn Host,
        candidates: BTreeMap<String, Vec<(String, Location)>>,
        reached: &mut Reached,
    ) -> Result<(), PluginError> {
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
            match reason(&report.location) {
                Some(reason) => self.leave(candidate, reason, entry),
                None => {
                    (reached.entry(candidate)).or_insert_with(|| (Via::BackRef, entry.clone()));
                }
            }
        }
        Ok(())
    }

    fn leave(&mut self, target: String, reason: Reason, entry: &str) {
        (self.left.entry(target)).or_insert_with(|| (reason, entry.to_owned()));
    }
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

/// The logical path and kind of each entry in this board's own tree that
/// matches `condition`.
fn entries(host: &dyn Host, condition: &str) -> Result<BTreeMap<String, String>, PluginError> {
    let sql =
        format!("SELECT logical, kind FROM entries WHERE source = 'worktree' AND ({condition})");
    let rows = query(host, &sql)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row.as_slice() {
            [Value::String(logical), Value::String(kind)] => Some((logical.clone(), kind.clone())),
            _ => None,
        })
        .collect())
}
