//! One run's decision about every entry (docs/architecture/index.md). A
//! snapshot refreshes the index, walks the boards reachable through mounts,
//! indexing each mounted commit once however many places mount it, decides
//! every entry and commits. What depends only on content comes from the
//! index; whether a reference or pointer resolves depends on the whole tree,
//! so each run decides it again, except for a mounted source whose decision
//! the index keeps under a key of all it depends on
//! (docs/architecture/performance/warm/decisions.md). The index's `entries` and `records` views
//! show what a snapshot decided, so they are read only through one. This
//! snapshot is a decided run over the index; `Repositories::snapshot` is a
//! different thing, a view of the git working tree for resolving paths.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use board_index::{Decided, Index, IndexError, Input, Resolved, Storage, View, Writer};
use board_model::{
    Change, Code, Digest, Kind, LogicalPath, ObjectId, REPO_TYPE, RecordType, Role, Source, Stat,
};
use board_tree::reads::Reads;
use board_tree::{
    Cache, Entry, Held, Listings, Mounted, Place, Probed, Repository, Scopes, Status, TreeRow,
    TreeRows, in_scope,
};

use crate::board::Board;
use crate::check::{CheckError, CheckReport};
use crate::index::{
    Indexed, Kept, Refresh, RefreshError, Refreshed, Run, Validating, attempt, facts, plain,
    transact, unvalidated, validated,
};

mod decide;
mod partial;

use decide::{Decide, Decisions};
pub(crate) use decide::{Referenced, read_type};
use partial::{Again, Heads, REMOTES, Slotted};

/// Every file each source's walk listed, sorted by path: the working tree
/// first, then each mounted commit.
pub(crate) type Walked = Vec<(Source, Vec<Entry>)>;

/// What one run decided about every entry it checked, and the index it
/// committed, whose views can show exactly that.
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub(crate) decided: Decided,
    pub(crate) report: CheckReport,
    /// Every reference each checked record makes, and where it led.
    pub(crate) references: Vec<Referenced>,
    pub(crate) walked: Walked,
}

/// What a snapshot is for, which decides whether it reads decisions the
/// index keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    /// Views of every entry. A mounted source whose decision the index
    /// keeps is neither walked nor decided, and its kept rows fill the views.
    View,
    /// A report, references and walk of every entry, all decided now.
    Report,
    /// A report only. A mounted source wholly in scope whose decision the
    /// index keeps is neither walked nor decided, and its kept rows fill
    /// the report.
    Check,
}

impl Need {
    /// Whether this need reads the decisions the index keeps.
    fn reads_kept(self) -> bool {
        match self {
            Self::View | Self::Check => true,
            Self::Report => false,
        }
    }
}

impl Board {
    /// Decides every entry of the working tree and of each board it mounts,
    /// or only those at or beneath `scope`, after refreshing `index`. An
    /// entry's status is the first problem found: its path, then parsing,
    /// then its schema, validation and references, or for a pointer its
    /// target.
    pub(crate) fn snapshot(
        &self,
        index: Index,
        scope: Option<&LogicalPath>,
        need: Need,
    ) -> Result<Snapshot, CheckError> {
        let _timed = self.timed("snapshot");
        let opened = self.repositories().opened();
        if let Some(scope) = scope {
            let _timed = self.timed("resolve");
            self.repositories()
                .resolve(&Source::WorkingTree, scope)
                .map_err(|source| CheckError::Scope { source })?;
        }
        let ((mut report, references, walked), wrote, writer, entries) =
            attempt(index, |writer| self.decide(writer, scope, need))?;
        let timed = self.timed("snapshot.commit");
        let decided = writer.decide(entries)?;
        drop(timed);
        report.work.wrote = wrote;
        report.work.repositories = self.repositories().opened() - opened;
        report.memory = match decided.storage() {
            Storage::Memory(fallback) => Some(*fallback),
            Storage::File(_) => None,
        };
        let work = &report.work;
        let mut trace = self.trace.borrow_mut();
        trace.count("entries", report.entries.len());
        trace.count("hashed", work.hashed);
        trace.count("parsed", work.parsed);
        trace.count("validated", work.validated);
        trace.count("trees", work.trees);
        trace.count("repositories", work.repositories);
        trace.count("pointers_read", work.pointers_read);
        trace.count("kept", work.kept);
        trace.count("walked", work.walked);
        trace.count("decided", work.decided);
        drop(trace);
        Ok(Snapshot {
            decided,
            report,
            references,
            walked,
        })
    }

    /// Fills the index's views with what a snapshot decided, for `query`,
    /// the one run that reads them.
    pub(crate) fn view(&self, decided: Decided) -> Result<View, IndexError> {
        let _timed = self.timed("view");
        decided.view()
    }

    /// Each mount in the working tree whose repository holds `directory`
    /// ([`board_tree::Repositories::places`]), its pointers read from the index after a
    /// refresh, which is all this decides.
    pub fn places(
        &self,
        directory: &Path,
        cache: Option<&Path>,
    ) -> Result<Vec<Place>, RefreshError> {
        let index = self.index(cache)?;
        let (places, _, _) = transact(index, |writer| {
            let refresh = self.refresh(writer)?;
            let is_pointer = |path: &str| Indexing::is_pointer(&refresh.facts, path);
            let is_repo = |path: &str| is_repo(&refresh.facts, path);
            let places =
                (self.repositories()).places(&refresh.entries, &is_pointer, &is_repo, directory);
            Ok((places, Vec::new()))
        })?;
        Ok(places)
    }

    /// The directory of each repository that a `board/mount/v1` record, or
    /// a `board/repo/v1` record with a `checkout`, in the working tree names
    /// ([`board_tree::Repositories::mounted_repositories`]), its pointers
    /// read from the index after a refresh, which is all this decides.
    pub fn mounted_repositories(&self, cache: Option<&Path>) -> Result<Vec<PathBuf>, RefreshError> {
        let index = self.index(cache)?;
        let (mounted, _, _) = transact(index, |writer| {
            let refresh = self.refresh(writer)?;
            let is_pointer = |path: &str| Indexing::is_pointer(&refresh.facts, path);
            let mounted = (self.repositories()).mounted_repositories(&refresh.entries, &is_pointer);
            Ok((mounted, Vec::new()))
        })?;
        Ok(mounted)
    }

    /// Decides each entry, and returns the report, the references, the
    /// files each source's walk listed and the rows of `resolved`.
    fn decide(
        &self,
        writer: &Writer,
        scope: Option<&LogicalPath>,
        need: Need,
    ) -> Run<(CheckReport, Vec<Referenced>, Walked)> {
        let Refresh {
            entries,
            listings,
            mut facts,
            kept,
            head,
            mut work,
        } = self.refresh(writer)?;
        let timed = self.timed("snapshot.mounts");
        let mounts = self.repositories().walk_mounts(
            &entries,
            &facts,
            &mut Indexing {
                writer,
                work: &mut work,
            },
            (self.build, need.reads_kept()),
            scope,
        )?;
        drop(timed);
        let _timed = self.timed("snapshot.decide");
        // A scoped run decides the working tree only in part.
        let slotted = match scope {
            None => self.slotted(writer, &mounts, (&entries, &listings), head)?,
            Some(_) => None,
        };
        let held = slotted.as_ref().is_some_and(Slotted::held);
        let mut decided = Decisions::default();
        let root = Source::WorkingTree;
        let validating =
            if let Some(slotted) = slotted.as_ref().filter(|_| held && need.reads_kept()) {
                unvalidated(&kept, &mut work);
                if need == Need::Check {
                    for row in writer.decision(&slotted.slot)? {
                        decided.checked.push(crate::check::from_kept(&root, row)?);
                    }
                }
                writer.reach(&root, Some(&slotted.slot), None)?;
                None
            } else {
                Some(Validating::new(self.repository(), kept))
            };
        let scopes = &mounts.scopes;
        let walked_facts = mounts
            .sources
            .iter()
            .filter_map(|m| Some(&m.walked.as_ref()?.1));
        let plain = plain(std::iter::once(&facts).chain(walked_facts));
        if let Some(mut validating) = validating {
            let listed = (entries.as_slice(), &mut facts, listings, head);
            let read = (writer, &plain, scopes, &mounts.cycles);
            let slotted = (slotted.as_ref(), held);
            let into = (&mut decided, &mut work);
            self.decide_slotted(read, listed, slotted, (need, &mut validating), into)?;
            validating.finish(&mut work);
        }
        for mounted in &mounts.sources {
            let read_kept = need.reads_kept() && mounted.kept && mounted.whole;
            // A source was not walked when its decision is kept or the scope
            // does not reach it.
            let Some(walked) = (mounted.walked.as_ref()).filter(|_| !read_kept) else {
                let source = Source::Commit(mounted.commit);
                let key = mounted.key.as_ref().filter(|_| mounted.kept);
                if let Some(key) = key.filter(|_| read_kept) {
                    work.kept += 1;
                    if need == Need::Check {
                        for row in writer.decision(key)? {
                            decided.checked.push(crate::check::from_kept(&source, row)?);
                        }
                    }
                }
                writer.reach(&source, key, Some(&mounted.scan))?;
                continue;
            };
            let read = (writer, &plain, scopes, &mounts.cycles);
            work.pointers_read += self.decide_mounted(read, mounted, walked, &mut decided)?;
        }
        record(&mounts, writer, self.repository())?;
        // Every source is now stored, so a prune keeps what this run reached.
        if writer.wrote()? {
            work.pruned = writer.prune_if_doubled()?;
        }
        work.decided = decided.decided;
        let report = CheckReport {
            entries: decided.checked,
            memory: None,
            work,
        };
        let mut walked = vec![(Source::WorkingTree, entries)];
        walked.extend(
            (mounts.sources.into_iter())
                .filter_map(|mounted| Some((Source::Commit(mounted.commit), mounted.walked?.0))),
        );
        Ok(((report, decided.references, walked), decided.resolved))
    }

    /// The key of the working tree's decision and the mounts it may cross
    /// ([`board_tree::Reached::worktree_key`]): it holds every entry the
    /// walk listed as a decision reads it, each directory and submodule the
    /// walk saw, which a resolution can reach though it holds no entry, the
    /// tree `HEAD` names, which says whether each entry is committed, and
    /// the moved prefixes a resolution tries.
    fn worktree_key(
        &self,
        mounts: &Mounts,
        (entries, listings): (&[Entry], &Listings),
        head: Option<ObjectId>,
    ) -> Option<(Digest, HashSet<board_tree::Crossing>)> {
        let head = head.map_or_default(|tree| tree.to_string());
        let moved = format!("{:?}", self.repositories().moved());
        let listed = listed(entries);
        let directories = seen_directories(listings);
        let fields: [&[u8]; 4] = [head.as_bytes(), moved.as_bytes(), &listed, &directories];
        mounts.worktree_key(self.build, self.repository(), &fields)
    }

    /// Decides the working tree, and keeps its rows in the slot when they
    /// may be kept: after a change, only the entries the change could alter
    /// when the slot allows ([`Board::again`]), else every entry. A run whose
    /// tree the slot holds (`held`) keeps nothing more.
    fn decide_slotted(
        &self,
        read: (&Writer, &HashSet<ObjectId>, &Scopes, &Cycles),
        (entries, facts, listings, head): (&[Entry], &mut Facts, Listings, Option<ObjectId>),
        (slotted, in_slot): (Option<&Slotted>, bool),
        (need, validating): (Need, &mut Validating<'_>),
        (decided, work): (&mut Decisions, &mut Refreshed),
    ) -> Result<(), RefreshError> {
        let writer = read.0;
        let partial = match slotted.filter(|_| need.reads_kept() && !in_slot) {
            Some(slotted) => Self::again(writer, slotted, entries, facts)?,
            None => None,
        };
        let partial = match (partial, slotted) {
            (Some(again), Some(slotted)) => {
                let heads = self.heads(writer, (slotted, head), (entries, &again.only), work)?;
                heads.map(|(heads, flipped)| (again, heads, flipped, slotted))
            }
            _ => None,
        };
        // A run whose tree the slot holds keeps nothing, so it records no
        // reads.
        let crossings = slotted
            .filter(|_| !in_slot)
            .map(|slotted| &slotted.crossings);
        if let Some((again, mut heads, flipped, slotted)) = partial {
            let Again { only, removed } = again;
            let plan = (Some(&only), crossings, &mut *validating);
            let listed = (entries, &mut *facts, listings, &mut heads);
            let rows = self.decide_worktree(read, listed, plan, decided, work)?;
            if rows.storable {
                let gone: Vec<&str> = (only.iter().chain(&removed)).map(String::as_str).collect();
                let kept = (&rows.inputs[..], &flipped[..]);
                Self::keep_slot(writer, (slotted, Some(&gone)), kept, decided)?;
                if need == Need::Check {
                    Self::report_slot(writer, slotted, decided)?;
                }
            }
            return Ok(());
        }
        let timed = self.timed("snapshot.head");
        let mut heads = Heads::Listed(self.head(writer, head, &mut work.trees)?);
        drop(timed);
        let listed = (entries, facts, listings, &mut heads);
        let plan = (None, crossings, validating);
        let rows = self.decide_worktree(read, listed, plan, decided, work)?;
        match slotted {
            Some(slotted) if in_slot => {
                decided.resolved.clear();
                writer.reach(&Source::WorkingTree, Some(&slotted.slot), None)?;
            }
            Some(slotted) if rows.storable => {
                let kept = (&rows.inputs[..], &[][..]);
                Self::keep_slot(writer, (slotted, None), kept, decided)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Decides each entry of the working tree in scope into `decided`, or
    /// with `only` each entry it names, checking each record it decides
    /// against its schema first, and says whether the rows may be kept:
    /// nothing failed to read, no two names share a path, no file could not
    /// be read, and each mount the resolutions crossed is among `crossings`.
    /// When the entries `only` names may not be kept, every other entry is
    /// decided too, and the rows are not kept. Only with `crossings` does it
    /// record what each decision read.
    fn decide_worktree(
        &self,
        (writer, plain, scopes, cycles): (&Writer, &HashSet<ObjectId>, &Scopes, &Cycles),
        (entries, facts, listings, heads): (&[Entry], &mut Facts, Listings, &mut Heads<'_>),
        (only, crossings, validating): (
            Option<&HashSet<String>>,
            Option<&HashSet<board_tree::Crossing>>,
            &mut Validating<'_>,
        ),
        decided: &mut Decisions,
        work: &mut Refreshed,
    ) -> Result<Rows, RefreshError> {
        let first = |path: &str| only.is_none_or(|only| only.contains(path));
        validating.validate(writer, facts, &first, &mut work.validated)?;
        let mut known = known(entries, &mut work.hashed);
        let reads = crossings.map(|_| Reads::default());
        let resolver = self.repositories().resolver(&mut known, listings)?;
        let resolver = resolver.knowing(plain);
        let mut resolver = match &reads {
            Some(reads) => resolver.reading(Rc::clone(reads)),
            None => resolver,
        };
        let wanted =
            |entry: &&Entry| in_scope(scopes, &Source::WorkingTree, entry.logical.as_ref());
        let whole = entries.iter().all(|entry| wanted(&entry));
        let mut inputs = Vec::new();
        let start = decided.resolved.len();
        let failed = {
            let facts = &*facts;
            let root = self.decider((writer, entries, facts, cycles), &mut resolver);
            for entry in (entries.iter().filter(wanted)).filter(|entry| first(&entry.path)) {
                let reads = reads.as_ref();
                Self::decide_entry(&root, (entry, facts), (heads, reads), decided, &mut inputs)?;
            }
            root.failed.get()
        };
        let crossed = resolver.crossed();
        let held = |crossed: &board_tree::Crossing| crossings.is_some_and(|c| c.contains(crossed));
        // The working tree's key holds no object store, so a file in it
        // that cannot be read keeps nothing.
        let mut storable = keepable(whole && !failed, &decided.resolved[start..])
            == Keepable::Whole
            && crossed.iter().all(held);
        if let Some(only) = only.filter(|_| !storable) {
            let rest = |path: &str| !only.contains(path);
            validating.validate(writer, facts, &rest, &mut work.validated)?;
            let facts = &*facts;
            let root = self.decider((writer, entries, facts, cycles), &mut resolver);
            let rest = |entry: &&Entry| !only.contains(&entry.path);
            for entry in entries.iter().filter(wanted).filter(rest) {
                let reads = reads.as_ref();
                Self::decide_entry(&root, (entry, facts), (heads, reads), decided, &mut inputs)?;
            }
            drop(root);
            decided.sort_from(start);
            storable = false;
        }
        work.pointers_read += resolver.pointers_read();
        Ok(Rows { storable, inputs })
    }

    /// What deciding the working tree's entries reads, resolving through
    /// `resolver`.
    fn decider<'d>(
        &'d self,
        (writer, entries, facts, cycles): (&'d Writer, &'d [Entry], &'d Facts, &'d Cycles),
        resolver: &'d mut board_tree::Resolver<'_, '_>,
    ) -> Decide<'d> {
        Decide {
            repositories: self.repositories(),
            mounted: None,
            writer,
            listed: entries,
            facts,
            resolve: RefCell::new(Box::new(move |path: &LogicalPath| resolver.resolve(path))),
            reader: RefCell::new(self.repositories().root().reader()),
            failed: Cell::new(false),
            remotes: OnceCell::new(),
            cycles,
        }
    }

    /// Decides the working tree's `entry` into `decided`. With `reads`, it
    /// records in `inputs` its digest and what its decision read: what
    /// `reads` holds of its resolutions, its schema's path and each
    /// directory above it, and for a repo record every repo record's
    /// `remote`.
    fn decide_entry(
        root: &Decide<'_>,
        (entry, facts): (&Entry, &Facts),
        (heads, reads): (&mut Heads<'_>, Option<&Reads>),
        decided: &mut Decisions,
        inputs: &mut Vec<Input>,
    ) -> Result<(), RefreshError> {
        let change = partial::change(heads, entry)?;
        let logical = entry.logical.clone();
        let Some(reads) = reads else {
            return Ok(decided.push(root, &Source::WorkingTree, entry, logical, change)?);
        };
        reads.borrow_mut().clear();
        decided.push(root, &Source::WorkingTree, entry, logical, change)?;
        let mut names = std::mem::take(&mut *reads.borrow_mut());
        let indexed = facts.get(&entry.path);
        if let Some(schema) = indexed.and_then(crate::index::schema_type) {
            let path = schema.schema_path();
            let above = path.rmatch_indices('/').map(|(at, _)| &path[..at]);
            names.extend(
                std::iter::once(&path[..])
                    .chain(above)
                    .map(board_tree::reads::named),
            );
        }
        if is_repo(facts, &entry.path) {
            names.push(REMOTES.to_owned());
        }
        names.sort_unstable();
        names.dedup();
        inputs.push(Input {
            path: entry.path.clone(),
            entry: partial::digest(entry),
            reads: names,
        });
        Ok(())
    }

    /// Decides each entry of `mounted` in scope into `decided`, and keeps
    /// its rows in the index under its key when they are its whole decision
    /// and nothing failed to read. Rows the index keeps leave `decided`'s
    /// rows, as the views read them from the index. Returns the pointers its
    /// resolver read.
    fn decide_mounted(
        &self,
        (writer, plain, scopes, cycles): (&Writer, &HashSet<ObjectId>, &Scopes, &Cycles),
        mounted: &Mounted<Facts>,
        (entries, facts): &(Vec<Entry>, Facts),
        decided: &mut Decisions,
    ) -> Result<usize, RefreshError> {
        let source = Source::Commit(mounted.commit);
        let repository = self.repositories().open_at(&mounted.repository)?;
        let in_commit = (self.repositories()).in_commit(&repository, mounted.commit)?;
        let mut in_commit = in_commit.knowing(plain);
        let decide = Decide {
            repositories: self.repositories(),
            mounted: Some(mounted),
            writer,
            listed: entries,
            facts,
            resolve: RefCell::new(Box::new(|path: &LogicalPath| in_commit.resolve(path))),
            reader: RefCell::new(self.repositories().root().reader()),
            failed: Cell::new(false),
            remotes: OnceCell::new(),
            cycles,
        };
        let start = decided.resolved.len();
        let checked = decided.checked.len();
        for entry in entries {
            if !in_scope(scopes, &source, entry.logical.as_ref()) {
                continue;
            }
            let logical = entry
                .logical
                .as_ref()
                .and_then(|logical| mounted.display(logical));
            decided.push(&decide, &source, entry, logical, Change::Committed)?;
        }
        let failed = decide.failed.get();
        drop(decide);
        // A mount no probe holds, such as one in a file not listed as a
        // pointer, may change without changing the key.
        let probed =
            (in_commit.crossed().iter()).all(|crossed| mounted.crossings.contains(crossed));
        let rows = &decided.resolved[start..];
        // Rows that hold a file that cannot be read are kept under the key
        // that also holds each object store they read.
        let storable = match keepable(mounted.whole && probed && !failed, rows) {
            Keepable::Whole => mounted.key.as_ref(),
            Keepable::Lacking => mounted.lacking.as_ref(),
            Keepable::Never => None,
        };
        let key = match (mounted.key.as_ref(), storable) {
            (Some(key), _) if mounted.kept => Some(key),
            (Some(_), Some(key)) => {
                let details = decided.checked[checked..].iter().map(crate::check::to_kept);
                writer.put_decision(key, rows.iter().zip(details))?;
                Some(key)
            }
            _ => None,
        };
        if key.is_some() {
            decided.resolved.truncate(start);
        }
        writer.reach(&source, key, Some(&mounted.scan))?;
        Ok(in_commit.pointers_read())
    }
}

/// The blob of each file `entries` saw as it is now, so it need not be
/// hashed again; each file it did not see as it is now counts in `hashed`.
fn known<'e>(
    entries: &'e [Entry],
    hashed: &'e mut usize,
) -> impl FnMut(&str, &Stat) -> Option<ObjectId> + 'e {
    let stats: HashMap<&str, (Stat, Option<ObjectId>)> = entries
        .iter()
        .filter_map(|entry| Some((entry.path.as_str(), (entry.stat?, entry.blob))))
        .collect();
    move |path, stat| {
        let seen = stats.get(path).filter(|(seen, _)| seen == stat);
        *hashed += usize::from(seen.is_none());
        seen.and_then(|(_, blob)| *blob)
    }
}

/// Whether a source's decided `rows` may be kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keepable {
    /// Under the source's key.
    Whole,
    /// Only under a key that also holds each object store the rows read,
    /// as one holds a file that could not be read, which may be read once
    /// a store changes.
    Lacking,
    Never,
}

/// Whether a source's decided `rows` may be kept, given that it was
/// decided whole and nothing failed to read (`whole`). Names that are not
/// UTF-8 can share a path, which the index keeps only once.
fn keepable(whole: bool, rows: &[Resolved]) -> Keepable {
    let mut paths = HashSet::new();
    if !whole || !rows.iter().all(|row| paths.insert(row.path.as_str())) {
        return Keepable::Never;
    }
    let lacks = |row: &Resolved| row.code == Some(Code::RepoUnreadable);
    if rows.iter().any(lacks) {
        Keepable::Lacking
    } else {
        Keepable::Whole
    }
}

/// Each directory and submodule `listings` saw, as the bytes of its path,
/// whether it is a submodule, and a separator.
fn seen_directories(listings: &Listings) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (path, submodule) in listings.seen_directories() {
        let path = path.as_os_str().as_encoded_bytes();
        bytes.extend(path.len().to_le_bytes());
        bytes.extend_from_slice(path);
        bytes.push(u8::from(submodule));
    }
    bytes
}

/// Each entry as deciding it reads it, without its metadata, each field
/// preceded by its length so that no two lists give the same bytes.
fn listed(entries: &[Entry]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut field = |text: &str| {
        bytes.extend(text.len().to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    };
    for entry in entries {
        let Entry {
            path,
            logical,
            blob,
            status,
            stat: _,
            size,
            link,
        } = entry;
        field(path);
        field(logical.as_ref().map_or("", LogicalPath::as_str));
        field(&blob.map_or_default(|blob| blob.to_string()));
        field(match status {
            Status::Ok => "",
            Status::Degraded(code) => code.as_str(),
        });
        field(&size.map_or_default(|size| size.to_string()));
        field(&link.as_ref().map_or_default(|link| link.blob.to_string()));
        field(link.as_ref().map_or("", |link| link.target.as_str()));
    }
    bytes
}

/// What the index holds about one source's entries, by physical path.
type Facts = HashMap<String, Indexed>;

/// Whether the working tree's rows may be kept, and each entry's input.
struct Rows {
    storable: bool,
    inputs: Vec<Input>,
}

/// Every mounted source, with what the index holds about its entries.
type Mounts = board_tree::Reached<Facts>;

/// Each mount pointer that would repeat a board above it, by source and
/// physical path, with that board's route.
type Cycles = HashMap<(Source, String), LogicalPath>;

/// The index as the cache a walk through mounts reads and fills.
struct Indexing<'w> {
    writer: &'w Writer,
    work: &'w mut Refreshed,
}

// A cache read that fails counts as a miss: the index is disposable.
impl Cache for Indexing<'_> {
    type Error = RefreshError;
    type Facts = Facts;

    fn has_tree(&mut self, tree: ObjectId) -> bool {
        self.writer.has_tree(tree).unwrap_or(false)
    }

    fn put_tree(&mut self, tree: &TreeRows) -> Result<(), RefreshError> {
        self.writer.put_tree(tree)?;
        self.work.trees += 1;
        Ok(())
    }

    fn rows(&mut self, tree: ObjectId) -> Option<Vec<TreeRow>> {
        self.writer.tree(tree).ok().flatten()
    }

    fn facts(
        &mut self,
        repository: &Repository,
        source: &Source,
        entries: &[Entry],
    ) -> Result<Facts, RefreshError> {
        // Each walk asks for its facts once.
        self.work.walked += 1;
        let mut kept = Kept::default();
        let mut facts = facts(
            self.writer,
            repository,
            source,
            entries,
            self.work,
            &mut kept,
        )?;
        validated(self.writer, repository, source, &mut facts, kept, self.work)?;
        Ok(facts)
    }

    fn is_pointer(facts: &Facts, path: &str) -> bool {
        facts.get(path).is_some_and(|indexed| {
            indexed.fact.kind == Kind::Pointer && indexed.fact.code.is_none()
        })
    }

    fn complete(facts: &Facts, entries: &[Entry]) -> bool {
        entries.iter().all(|entry| {
            entry.blob.is_none()
                || Role::of(&entry.path).is_none()
                || facts.contains_key(&entry.path)
        })
    }

    fn pointers(&mut self, scan: &Digest) -> Option<Vec<Held>> {
        self.writer.pointers(scan).ok().flatten()
    }

    fn put_pointers(&mut self, scan: &Digest, held: &[Held]) -> Result<(), RefreshError> {
        Ok(self.writer.put_pointers(scan, held)?)
    }

    fn kept(&mut self, key: &Digest) -> bool {
        self.writer.has_decision(key).unwrap_or(false)
    }

    fn probe(&mut self, stamp: &Digest) -> Option<Probed> {
        self.writer.probe(stamp).ok().flatten()
    }

    fn put_probe(&mut self, stamp: &Digest, probed: &Probed) -> Result<(), RefreshError> {
        Ok(self.writer.put_probe(stamp, probed)?)
    }
}

/// Whether the entry at the physical `path` holds a valid repo record, with
/// a checkout or without.
fn is_repo(facts: &Facts, path: &str) -> bool {
    facts.get(path).is_some_and(|indexed| {
        let fact = &indexed.fact;
        fact.code.is_none() && fact.record_type.as_ref().map(RecordType::as_str) == Some(REPO_TYPE)
    })
}

/// Stores every source, the root's repository first, and every mount.
fn record(mounts: &Mounts, writer: &Writer, root: &Repository) -> Result<(), IndexError> {
    let mut sources = vec![(
        Source::WorkingTree,
        root.head_tree().ok().flatten(),
        root.path(),
    )];
    sources.extend(mounts.sources.iter().map(|mounted| {
        (
            Source::Commit(mounted.commit),
            Some(mounted.root),
            mounted.repository.as_path(),
        )
    }));
    writer.set_mounts(&sources, &mounts.points)
}

#[cfg(test)]
mod tests;
