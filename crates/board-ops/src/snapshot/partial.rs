//! The working tree's decision kept in a slot, and deciding again after a
//! change only the entries the change could alter
//! (docs/architecture/performance/warm/index.md).

use std::collections::{HashMap, HashSet};

use board_index::{IndexError, Input, Slot, Writer};
use board_model::{Change, Digest, ObjectId, Source};
use board_tree::reads::{ANY, changed};
use board_tree::{Crossing, Entry, Listings, TreeRow};

use super::{Decisions, Facts, Mounts, is_repo, listed};
use crate::board::Board;
use crate::index::{RefreshError, Refreshed};

/// The read a repo record's decision records: it reads every repo record's
/// `remote`.
pub(super) const REMOTES: &str = "#remotes";

/// The most entries, as a share of all, a run decides again in place of
/// deciding every entry: one in twenty.
const SHARE: usize = 20;

/// What a run knows of the working tree's slot.
pub(super) struct Slotted {
    /// Where the build keeps the board root's decision.
    pub(super) slot: Digest,
    /// Each mount the working tree's decision may cross, because its key
    /// holds that mount's probe.
    pub(super) crossings: HashSet<Crossing>,
    /// What the slot holds, when it holds anything.
    pub(super) kept: Option<Slot>,
    /// What the slot holds once it keeps this run's decision.
    pub(super) now: Slot,
}

impl Slotted {
    /// Whether the slot holds this run's tree.
    pub(super) fn held(&self) -> bool {
        self.kept
            .as_ref()
            .is_some_and(|kept| kept.key == self.now.key)
    }
}

/// The blob each path has in `HEAD`'s tree, read as a run needs it.
pub(super) enum Heads<'w> {
    /// Every file, listed at once.
    Listed(HashMap<String, Option<ObjectId>>),
    /// Read from the index's tree rows, one path at a time.
    Trees {
        writer: &'w Writer,
        root: Option<ObjectId>,
        rows: HashMap<ObjectId, HashMap<String, (u32, ObjectId)>>,
    },
}

impl Heads<'_> {
    /// Whether `HEAD`'s tree holds `own` at the `/`-separated `path`: a
    /// file or symlink with that blob, or a submodule when `own` is `None`.
    pub(super) fn holds(&mut self, path: &str, own: Option<ObjectId>) -> Result<bool, IndexError> {
        let (writer, root, rows) = match self {
            Self::Listed(files) => return Ok(files.get(path) == Some(&own)),
            Self::Trees { writer, root, rows } => (writer, root, rows),
        };
        let Some(mut tree) = *root else {
            return Ok(false);
        };
        let mut segments = path.split('/').peekable();
        while let Some(segment) = segments.next() {
            let Some((mode, object)) = named(writer, rows, tree)?.get(segment).copied() else {
                return Ok(false);
            };
            match (mode == TreeRow::TREE_MODE, segments.peek().is_some()) {
                (true, true) => tree = object,
                (false, false) if mode == SUBMODULE_MODE => return Ok(own.is_none()),
                (false, false) => return Ok(own == Some(object)),
                _ => return Ok(false),
            }
        }
        Ok(false)
    }
}

/// Each path whose `change` a run sets again without deciding it.
pub(super) type Flipped = Vec<(String, Change)>;

/// git's mode for a submodule.
const SUBMODULE_MODE: u32 = 0o160_000;

/// The rows of `tree` by name, as the index's `files` names them, read once.
fn named<'r>(
    writer: &Writer,
    rows: &'r mut HashMap<ObjectId, HashMap<String, (u32, ObjectId)>>,
    tree: ObjectId,
) -> Result<&'r HashMap<String, (u32, ObjectId)>, IndexError> {
    Ok(match rows.entry(tree) {
        std::collections::hash_map::Entry::Occupied(held) => held.into_mut(),
        std::collections::hash_map::Entry::Vacant(vacant) => {
            let read = writer.tree(tree)?.unwrap_or_default();
            let by_name = (read.into_iter())
                .map(|row| {
                    let name = String::from_utf8_lossy(&row.name).into_owned();
                    (name, (row.mode, row.object))
                })
                .collect();
            vacant.insert(by_name)
        }
    })
}

/// Each `/`-separated path whose entry differs between the trees `old` and
/// `new`, read from the index's tree rows, or `None` when it does not hold
/// one of them.
fn head_changes(
    writer: &Writer,
    (old, new): (Option<ObjectId>, Option<ObjectId>),
) -> Result<Option<Vec<String>>, IndexError> {
    let mut changes = Vec::new();
    let mut pending = vec![(String::new(), old, new)];
    while let Some((prefix, old, new)) = pending.pop() {
        if old == new {
            continue;
        }
        let mut sides = [HashMap::new(), HashMap::new()];
        for (side, tree) in sides.iter_mut().zip([old, new]) {
            let Some(tree) = tree else { continue };
            let Some(rows) = writer.tree(tree)? else {
                return Ok(None);
            };
            for row in rows {
                let name = String::from_utf8_lossy(&row.name).into_owned();
                side.insert(name, (row.mode, row.object));
            }
        }
        let names: HashSet<&String> = sides[0].keys().chain(sides[1].keys()).collect();
        for name in names {
            let [before, after] = [&sides[0], &sides[1]].map(|side| side.get(name).copied());
            if before == after {
                continue;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let tree = |side: Option<(u32, ObjectId)>| {
                side.filter(|(mode, _)| *mode == TreeRow::TREE_MODE)
                    .map(|(_, id)| id)
            };
            let [old_tree, new_tree] = [tree(before), tree(after)];
            if old_tree.is_some() || new_tree.is_some() {
                pending.push((path.clone(), old_tree, new_tree));
            }
            if old_tree.is_none() || new_tree.is_none() {
                changes.push(path);
            }
        }
    }
    Ok(Some(changes))
}

/// The digest of `entry` as deciding reads it.
pub(super) fn digest(entry: &Entry) -> String {
    Digest::of(&listed(std::slice::from_ref(entry))).hex()
}

/// What a run decides again after a change: the paths to decide and those
/// the slot holds that the walk no longer lists.
pub(super) struct Again {
    pub(super) only: HashSet<String>,
    pub(super) removed: Vec<String>,
}

impl Board {
    /// The working tree's slot and what it holds, when its decision has a
    /// key ([`Board::worktree_key`]).
    pub(super) fn slotted(
        &self,
        writer: &Writer,
        mounts: &Mounts,
        (entries, listings): (&[Entry], &Listings),
        head: Option<ObjectId>,
    ) -> Result<Option<Slotted>, RefreshError> {
        let Some((key, crossings)) = self.worktree_key(mounts, (entries, listings), head) else {
            return Ok(None);
        };
        let Some(probes) = mounts.worktree_probes() else {
            return Ok(None);
        };
        let root = self.repository().path().as_os_str().as_encoded_bytes();
        let mut bytes = b"slot\0".to_vec();
        bytes.extend_from_slice(self.build.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(root);
        let slot = Digest::of(&bytes);
        let moved = format!("{:?}", self.repositories().moved());
        let directories = (listings.seen_directories().into_iter())
            .map(|(path, submodule)| {
                let kind = if submodule { "s:" } else { "d:" };
                format!("{kind}{}", path.to_string_lossy())
            })
            .collect();
        let now = Slot {
            key: key.hex(),
            frame: Digest::of(moved.as_bytes()).hex(),
            head,
            directories,
            probes: (probes.into_iter())
                .map(|(path, probe)| (path, probe.hex()))
                .collect(),
        };
        Ok(Some(Slotted {
            kept: writer.slot(&slot)?,
            slot,
            crossings,
            now,
        }))
    }

    /// What to decide again after a change: each entry that changed, was
    /// added, is a pointer whose probe changed, or read a name a change
    /// touched. `None` when every entry is
    /// decided: the slot holds nothing or another frame, two entries share
    /// a path, or more than one entry in [`SHARE`] would be decided.
    pub(super) fn again(
        writer: &Writer,
        slotted: &Slotted,
        entries: &[Entry],
        facts: &Facts,
    ) -> Result<Option<Again>, RefreshError> {
        let Some(kept) = slotted
            .kept
            .as_ref()
            .filter(|k| k.frame == slotted.now.frame)
        else {
            return Ok(None);
        };
        let inputs = writer.inputs(&slotted.slot)?;
        if inputs.is_empty() {
            return Ok(None);
        }
        let mut old: HashMap<&str, &Input> = (inputs.iter())
            .map(|input| (input.path.as_str(), input))
            .collect();
        let mut touched: HashSet<String> = HashSet::new();
        let mut only = HashSet::new();
        let mut paths = HashSet::new();
        let touch = |touched: &mut HashSet<String>, path: &str, was: Option<&Input>| {
            touched.extend(changed(path));
            if was.is_some_and(|was| was.reads.iter().any(|read| read == REMOTES)) {
                touched.insert(REMOTES.to_owned());
            }
        };
        for entry in entries {
            let path = entry.path.as_str();
            if !paths.insert(path) {
                return Ok(None);
            }
            let was = old.remove(path);
            if was.is_some_and(|was| was.entry == digest(entry)) {
                continue;
            }
            touch(&mut touched, path, was);
            if is_repo(facts, path) {
                touched.insert(REMOTES.to_owned());
            }
            only.insert(entry.path.clone());
        }
        let removed: Vec<String> = old.keys().map(|path| (*path).to_owned()).collect();
        for (path, was) in old {
            touch(&mut touched, path, Some(was));
        }
        let [before, after] = [&kept.directories, &slotted.now.directories]
            .map(|directories| directories.iter().collect::<HashSet<_>>());
        for directory in before.symmetric_difference(&after) {
            touched.extend(changed(directory.get(2..).unwrap_or_default()));
        }
        let [before, after] =
            [&kept.probes, &slotted.now.probes].map(|probes| probes.iter().collect::<HashSet<_>>());
        // A pointer whose probe changed is decided again itself, because a
        // decision that stops at a cycle reads no name.
        for (path, _) in before.symmetric_difference(&after) {
            touched.extend(changed(path));
            if paths.contains(path.as_str()) {
                only.insert(path.clone());
            }
        }
        if !touched.is_empty() {
            for input in &inputs {
                let reads = |read: &String| read == ANY || touched.contains(read);
                if paths.contains(input.path.as_str()) && input.reads.iter().any(reads) {
                    only.insert(input.path.clone());
                }
            }
        }
        if only.len() * SHARE > entries.len() {
            return Ok(None);
        }
        Ok(Some(Again { only, removed }))
    }

    /// `HEAD`'s tree read one path at a time, with the `change` of each
    /// entry not in `only` whose `HEAD` blob differs from the slot's
    /// `HEAD`. `None` when the index does not hold the slot's `HEAD` tree.
    pub(super) fn heads<'w>(
        &self,
        writer: &'w Writer,
        (slotted, head): (&Slotted, Option<ObjectId>),
        (entries, only): (&[Entry], &HashSet<String>),
        work: &mut Refreshed,
    ) -> Result<Option<(Heads<'w>, Flipped)>, RefreshError> {
        let _timed = self.timed("snapshot.head");
        if let Some(root) = head {
            self.store_head(writer, root, &mut work.trees)?;
        }
        let old = slotted.kept.as_ref().and_then(|kept| kept.head);
        let Some(changes) = head_changes(writer, (old, head))? else {
            return Ok(None);
        };
        let mut heads = Heads::Trees {
            writer,
            root: head,
            rows: HashMap::new(),
        };
        let by_path: HashMap<&str, &Entry> = (entries.iter())
            .map(|entry| (entry.path.as_str(), entry))
            .collect();
        let mut flipped = Vec::new();
        for path in changes.iter().filter(|path| !only.contains(*path)) {
            let Some(entry) = by_path.get(path.as_str()) else {
                continue;
            };
            flipped.push((path.clone(), change(&mut heads, entry)?));
        }
        Ok(Some((heads, flipped)))
    }

    /// Keeps in the slot what this run decided: in place of the rows and
    /// inputs of `gone` when given, else in place of all, with the `change`
    /// of each of `flipped`. Kept rows leave `decided`'s rows, as the views
    /// read them from the index.
    pub(super) fn keep_slot(
        writer: &Writer,
        (slotted, gone): (&Slotted, Option<&[&str]>),
        (inputs, flipped): (&[Input], &[(String, Change)]),
        decided: &mut Decisions,
    ) -> Result<(), RefreshError> {
        let slot = &slotted.slot;
        writer.forget(slot, gone)?;
        let details = decided.checked.iter().map(crate::check::to_kept);
        writer.put_decision(slot, decided.resolved.iter().zip(details))?;
        writer.put_inputs(slot, inputs)?;
        for (path, change) in flipped {
            writer.set_change(slot, path, *change)?;
        }
        writer.put_slot(slot, &slotted.now, slotted.kept.is_some())?;
        decided.resolved.clear();
        writer.reach(&Source::WorkingTree, Some(slot), None)?;
        Ok(())
    }

    /// Replaces what `decided` reports of the working tree with every row
    /// the slot keeps, as a check of a tree the slot holds reports it.
    pub(super) fn report_slot(
        writer: &Writer,
        slotted: &Slotted,
        decided: &mut Decisions,
    ) -> Result<(), RefreshError> {
        decided.checked.clear();
        for row in writer.decision(&slotted.slot)? {
            (decided.checked).push(crate::check::from_kept(&Source::WorkingTree, row)?);
        }
        Ok(())
    }
}

/// Whether `entry` is committed: `HEAD` holds its own blob at its path.
/// A symlink is committed when its own blob is.
pub(super) fn change(heads: &mut Heads<'_>, entry: &Entry) -> Result<Change, IndexError> {
    let own = entry.link.as_ref().map(|link| link.blob).or(entry.blob);
    Ok(if heads.holds(&entry.path, own)? {
        Change::Committed
    } else {
        Change::Uncommitted
    })
}
