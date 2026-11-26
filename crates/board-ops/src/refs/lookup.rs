//! Where each object's references sit in one snapshot, so `refs` answers
//! each path by a lookup instead of a scan of every reference, entry and
//! walked file (docs/architecture/index.md).

use std::cell::OnceCell;
use std::collections::HashMap;

use board_model::{Code, Source};
use board_tree::{Entry, Status};

use crate::snapshot::Snapshot;

/// An object's identity in a snapshot: its source and physical path.
type At = (Source, String);

/// The places in one snapshot's `references` of those each object makes
/// and of those that reach it, each list in the snapshot's order, and,
/// built when a run first reads a target, each degraded entry's code and
/// each walked file's blob. Every lookup must pass the snapshot the lookup
/// was built from.
#[derive(Debug, Default)]
pub(super) struct Lookup {
    from: HashMap<At, Vec<usize>>,
    to: HashMap<At, Vec<usize>>,
    targets: OnceCell<Targets>,
}

/// What `refs` reads about a reference's target, by the target's identity.
#[derive(Debug, Default)]
struct Targets {
    /// The first degraded entry's code at each identity.
    degraded: HashMap<At, Code>,
    /// Where each file the walk of its source listed is in the snapshot's
    /// `walked`, from the first walk of that source: the source's place,
    /// then the file's.
    walked: HashMap<At, (usize, usize)>,
}

impl Lookup {
    pub(super) fn new(snapshot: &Snapshot) -> Self {
        let mut lookup = Self::default();
        for (n, referenced) in snapshot.references.iter().enumerate() {
            let made = (referenced.source.clone(), referenced.path.clone());
            lookup.from.entry(made).or_default().push(n);
            if let Some(reached) = &referenced.reached.location {
                let at = (reached.board.source.clone(), reached.path.clone());
                lookup.to.entry(at).or_default().push(n);
            }
        }
        lookup
    }

    /// The places of the references the object makes.
    pub(super) fn from(&self, source: &Source, path: &str) -> &[usize] {
        Self::listed(&self.from, source, path)
    }

    /// The places of the references that resolve to the object.
    pub(super) fn to(&self, source: &Source, path: &str) -> &[usize] {
        Self::listed(&self.to, source, path)
    }

    /// The object's code when `snapshot` decided it degraded.
    pub(super) fn degraded(
        &self,
        snapshot: &Snapshot,
        source: &Source,
        path: &str,
    ) -> Option<Code> {
        let at = (source.clone(), path.to_owned());
        self.targets(snapshot).degraded.get(&at).copied()
    }

    /// The file the walk of the object's source listed at its path, or
    /// `None` when that source was not walked or listed no file there.
    pub(super) fn walked<'a>(
        &self,
        snapshot: &'a Snapshot,
        source: &Source,
        path: &str,
    ) -> Option<&'a Entry> {
        let at = (source.clone(), path.to_owned());
        let (walk, file) = *self.targets(snapshot).walked.get(&at)?;
        Some(&snapshot.walked[walk].1[file])
    }

    fn listed<'a>(map: &'a HashMap<At, Vec<usize>>, source: &Source, path: &str) -> &'a [usize] {
        map.get(&(source.clone(), path.to_owned()))
            .map_or(&[], Vec::as_slice)
    }

    fn targets(&self, snapshot: &Snapshot) -> &Targets {
        self.targets.get_or_init(|| {
            let mut targets = Targets::default();
            for entry in &snapshot.report.entries {
                if let Status::Degraded(code) = entry.status {
                    let at = (entry.source.clone(), entry.path.clone());
                    targets.degraded.entry(at).or_insert(code);
                }
            }
            for (walk, (source, entries)) in snapshot.walked.iter().enumerate() {
                for (file, entry) in entries.iter().enumerate() {
                    let at = (source.clone(), entry.path.clone());
                    targets.walked.entry(at).or_insert((walk, file));
                }
            }
            targets
        })
    }
}
