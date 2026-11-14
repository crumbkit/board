//! The local step of a sync: a three-way merge of the common ancestor, the
//! local HEAD and the fetched commit, made into a two-parent commit, then
//! HEAD, the index and the working tree move to it
//! (docs/architecture/writes.md, Sync).

use gix::bstr::BString;
use std::path::PathBuf;

use board_model::{
    CONFLICT_TYPE, CONFLICTS_DIRECTORY, Conflict, ConflictKind, Mount, ObjectId, RecordType, Side,
    Version, canonical_json, is_json_file, parse_json,
};
use board_validate::Schema;
use gix::object::tree::EntryKind;

use super::merge::{self, Ancestry};
use super::{Conflicted, Integrated, SyncError};
use crate::git::{Repository, model_id};
use crate::transport::{Deadline, Upstream};
use crate::{LOCK_WAIT, WriteError};

/// What a merge left besides its commit.
#[derive(Default)]
struct Outcome {
    conflicts: Vec<Conflicted>,
    pins: Vec<merge::PinResolution>,
    failing: Vec<String>,
}

impl Repository {
    /// The local step of a sync, under the board lock: merges `fetched`
    /// into HEAD, then moves HEAD, the index and the working tree to the
    /// result. HEAD stays when it already holds `fetched`, and moves to
    /// `fetched` when it is behind it; otherwise a merge commit with HEAD
    /// as its first parent and `fetched` as its second holds the merge,
    /// and each conflict as a `board/conflict/v1` record. The lock is not
    /// held across any network call.
    pub(super) fn integrate(
        &self,
        upstream: &Upstream,
        fetched: ObjectId,
        before: super::refuse::Before,
        deadline: Deadline,
    ) -> Result<Integrated, SyncError> {
        self.refuse_unsafe(upstream, fetched, before)?;
        let lock = self
            .lock(deadline.left().min(LOCK_WAIT))
            .map_err(SyncError::from)?;
        let old = self.head()?;
        let (new, merged, outcome) = match old {
            None => (fetched, false, Outcome::default()),
            Some(head) => self.merge_into(upstream, head, fetched)?,
        };
        let kept = if old == Some(new) {
            Vec::new()
        } else {
            let changes = self.changes(&self.files_of(old)?, &self.files_of(Some(new))?)?;
            self.check_out(&lock, old, new, changes)?
        };
        Ok(Integrated {
            old,
            new,
            merged,
            conflicts: outcome.conflicts,
            pins: outcome.pins,
            failing: outcome.failing,
            kept,
        })
    }

    /// The commit HEAD should move to, and whether it is a merge commit
    /// this made. Writes objects only: no ref, file or index.
    fn merge_into(
        &self,
        upstream: &Upstream,
        head: ObjectId,
        fetched: ObjectId,
    ) -> Result<(ObjectId, bool, Outcome), SyncError> {
        let (local, remote) = (self.gix_id(head)?, self.gix_id(fetched)?);
        let base = self.merge_base(local, remote)?;
        if base == Some(remote) {
            return Ok((head, false, Outcome::default()));
        }
        if base == Some(local) {
            return Ok((fetched, false, Outcome::default()));
        }
        let git = self.git();
        let identity = |found: Option<Result<gix::actor::SignatureRef<'_>, _>>| {
            found
                .ok_or(WriteError::NoIdentity)?
                .map_err(|source| self.unreadable(source))?
                .to_owned()
                .map_err(|source| self.unreadable(source))
        };
        let author = identity(git.author().map(|a| a.map_err(Box::new)))?;
        let committer = identity(git.committer().map(|c| c.map_err(Box::new)))?;
        let base_tree = base.map(|base| self.tree_of(base)).transpose()?;
        let (local_tree, remote_tree) = (self.tree_of(local)?, self.tree_of(remote)?);
        let home = std::env::home_dir();
        let descends = |mount: &Mount, candidate: ObjectId, of: ObjectId| {
            self.descends_in(home.as_deref(), mount, candidate, of)
        };
        let merged = merge::merge_trees(self, base_tree, local_tree, remote_tree, &descends)?;
        let mut outcome = Outcome {
            pins: merged.pins,
            ..Outcome::default()
        };
        let mut tree = self.gix_id(merged.tree)?;
        if !merged.conflicts.is_empty() {
            let mut editor = git
                .edit_tree(tree)
                .map_err(|source| self.unreadable(source))?;
            for found in &merged.conflicts {
                let record = self.conflict_record(found, local, remote, base)?;
                let path = format!("{CONFLICTS_DIRECTORY}{}.json", self.name_of(&record));
                // A record that is already there is about this same
                // conflict, and is not replaced, so it keeps its commits.
                if self.entry_in(tree, &path)?.is_none() {
                    let bytes = canonical_json(&record.to_value()).into_bytes();
                    let blob = git
                        .write_blob(&bytes)
                        .map_err(|source| self.unreadable(source))?
                        .detach();
                    editor
                        .upsert(path.as_str(), EntryKind::Blob, blob)
                        .map_err(|source| self.unreadable(source))?;
                }
                outcome.conflicts.push(Conflicted {
                    path: found.path.clone(),
                    kind: record.kind,
                    record: path,
                });
            }
            tree = editor
                .write()
                .map_err(|source| self.unreadable(source))?
                .detach();
        }
        outcome.failing = self.failing_records(&merged.schemas, base_tree, local_tree)?;
        let message = format!(
            "board sync: merge {}/{}\n",
            upstream.remote, upstream.branch
        );
        let commit = git
            .write_object(gix::objs::Commit {
                tree,
                parents: [local, remote].into_iter().collect(),
                author,
                committer,
                encoding: None,
                message: message.into(),
                extra_headers: Vec::new(),
            })
            .map_err(|source| self.unreadable(source))?
            .detach();
        Ok((model_id(&commit), true, outcome))
    }

    /// The best common ancestor of two commits, `None` when they share
    /// none.
    fn merge_base(
        &self,
        local: gix::ObjectId,
        remote: gix::ObjectId,
    ) -> Result<Option<gix::ObjectId>, SyncError> {
        common_ancestor(self.git(), local, remote).map_err(|source| self.unreadable(source))
    }

    /// A name for the record of a conflict: the hash of its path and the
    /// three blobs in dispute (the ancestor's and each side's, or `-` for
    /// none), the sides in order of id. The same conflict always has the same name, and a later
    /// conflict at the same path, over other content, has another.
    fn name_of(&self, record: &Conflict) -> String {
        let blob =
            |blob: Option<ObjectId>| blob.map_or_else(|| "-".to_owned(), |id| id.to_string());
        // The two sides in order of id, so the same pair merged with the
        // sides swapped, by two clones each the other's remote, is one name.
        let mut sides = [blob(record.local.blob), blob(record.remote.blob)];
        sides.sort();
        let key = format!(
            "{}\n{}\n{}\n{}",
            record.path,
            blob(record.base),
            sides[0],
            sides[1]
        );
        self.blob_id(key.as_bytes()).to_string()
    }

    /// The record of `found`, a conflict between `local` and `remote`
    /// whose common ancestor is `base`.
    fn conflict_record(
        &self,
        found: &merge::Conflict,
        local: gix::ObjectId,
        remote: gix::ObjectId,
        base: Option<gix::ObjectId>,
    ) -> Result<Conflict, SyncError> {
        let (kind, kept, held) = match found.kind {
            merge::ConflictKind::BothChanged { local, remote } => (
                ConflictKind::BothChanged,
                Side::Remote,
                (Some(local), Some(remote)),
            ),
            merge::ConflictKind::EditDelete {
                edited,
                by: Side::Local,
            } => (ConflictKind::EditDelete, Side::Local, (Some(edited), None)),
            merge::ConflictKind::EditDelete {
                edited,
                by: Side::Remote,
            } => (ConflictKind::EditDelete, Side::Remote, (None, Some(edited))),
            merge::ConflictKind::FileDirectory {
                file,
                by: Side::Local,
            } => (
                ConflictKind::FileDirectory,
                Side::Remote,
                (Some(file), None),
            ),
            merge::ConflictKind::FileDirectory {
                file,
                by: Side::Remote,
            } => (
                ConflictKind::FileDirectory,
                Side::Remote,
                (None, Some(file)),
            ),
        };
        Ok(Conflict {
            path: found.path.clone(),
            kind,
            kept,
            base: found.base,
            local: self.last_change(local, base, &found.path, held.0)?,
            remote: self.last_change(remote, base, &found.path, held.1)?,
        })
    }

    /// Who last changed `path` on the side whose tip is `tip`, looking back
    /// to `base`: the newest commit whose entry at `path` differs from its
    /// first parent's. A side with no such commit (a shallow history) is
    /// credited to its tip.
    fn last_change(
        &self,
        tip: gix::ObjectId,
        base: Option<gix::ObjectId>,
        path: &str,
        blob: Option<ObjectId>,
    ) -> Result<Version, SyncError> {
        let git = self.git();
        let walk = git
            .rev_walk([tip])
            .with_hidden(base)
            .all()
            .map_err(|source| self.unreadable(source))?;
        let mut found = tip;
        for info in walk {
            let info = info.map_err(|source| self.unreadable(source))?;
            let now = self.entry_at(info.id, path)?;
            let before = match info.parent_ids.first() {
                Some(parent) => self.entry_at(*parent, path)?,
                None => None,
            };
            if now != before {
                found = info.id;
                break;
            }
        }
        let commit = git
            .find_commit(found)
            .map_err(|source| self.unreadable(source))?;
        let author = commit.author().map_err(|source| self.unreadable(source))?;
        Ok(Version {
            blob,
            commit: model_id(&found),
            author: format!("{} <{}>", author.name, author.email),
        })
    }

    /// The object `commit`'s tree holds at `path`.
    fn entry_at(
        &self,
        commit: gix::ObjectId,
        path: &str,
    ) -> Result<Option<gix::ObjectId>, SyncError> {
        self.entry_in(self.tree_of(commit)?, path)
    }

    /// The object the tree `tree` holds at `path`.
    fn entry_in(
        &self,
        mut tree: gix::ObjectId,
        path: &str,
    ) -> Result<Option<gix::ObjectId>, SyncError> {
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let found = self
                .git()
                .find_tree(tree)
                .map_err(|source| self.unreadable(source))?;
            let decoded = found.decode().map_err(|source| self.unreadable(source))?;
            let Some(entry) = decoded.entries.iter().find(|entry| entry.filename == part) else {
                return Ok(None);
            };
            if parts.peek().is_none() {
                return Ok(Some(entry.oid.to_owned()));
            }
            if !entry.mode.is_tree() {
                return Ok(None);
            }
            tree = entry.oid.to_owned();
        }
        Ok(None)
    }

    /// Whether `candidate` has `of` among its ancestors in the repository
    /// `mount` names, read from `home` for a `~/` path.
    fn descends_in(
        &self,
        home: Option<&std::path::Path>,
        mount: &Mount,
        candidate: ObjectId,
        of: ObjectId,
    ) -> Ancestry {
        let path: PathBuf = match mount.repo.strip_prefix("~/") {
            Some(rest) => match home {
                Some(home) => home.join(rest),
                None => return Ancestry::Unknown,
            },
            None => self.path().join(&mount.repo),
        };
        let Ok(other) = gix::open(path) else {
            return Ancestry::Unknown;
        };
        let ids = (
            gix::ObjectId::try_from(candidate.as_bytes()),
            gix::ObjectId::try_from(of.as_bytes()),
        );
        let (Ok(candidate), Ok(of)) = ids else {
            return Ancestry::Unknown;
        };
        if other.find_commit(candidate).is_err() || other.find_commit(of).is_err() {
            return Ancestry::Unknown;
        }
        match common_ancestor(&other, candidate, of) {
            Ok(Some(base)) if base == of => Ancestry::Descends,
            Ok(_) => Ancestry::DoesNot,
            Err(_) => Ancestry::Unknown,
        }
    }

    /// The records the local side added or changed since `base` that
    /// fail a schema both sides changed, as the remote's schema reads
    /// them: their paths in the repository.
    fn failing_records(
        &self,
        clashes: &[merge::SchemaClash],
        base: Option<gix::ObjectId>,
        local: gix::ObjectId,
    ) -> Result<Vec<String>, SyncError> {
        if clashes.is_empty() {
            return Ok(Vec::new());
        }
        let before = match base {
            Some(base) => self.files(base)?,
            None => super::Files::new(),
        };
        let after = self.files(local)?;
        let mut failing = Vec::new();
        for clash in clashes {
            let Some(record_type) = RecordType::of_schema(&clash.path) else {
                continue;
            };
            let Some(bytes) = self.read_blob(clash.remote)? else {
                continue;
            };
            let Ok(schema) = parse_json(&bytes)
                .map_err(drop)
                .and_then(|document| Schema::compile(document).map_err(drop))
            else {
                continue;
            };
            for (path, entry) in &after {
                let path = path.to_string();
                if before.get(&BString::from(path.as_bytes())) == Some(entry)
                    || !is_json_file(&path)
                {
                    continue;
                }
                let Some(bytes) = self.read_blob(model_id(&entry.0))? else {
                    continue;
                };
                let Ok(record) = parse_json(&bytes) else {
                    continue;
                };
                let is_type = record.get("$type").and_then(|tag| tag.as_str());
                if is_type == Some(record_type.as_str())
                    && is_type != Some(CONFLICT_TYPE)
                    && schema.validate(&record).is_err()
                {
                    failing.push(path);
                }
            }
        }
        failing.sort();
        failing.dedup();
        Ok(failing)
    }
}

/// The best common ancestor of two commits, `None` when their histories
/// share no commit. gix reports both that case and a damaged history as an
/// error, so an error is settled by walking both histories.
pub(super) fn common_ancestor(
    git: &gix::Repository,
    one: gix::ObjectId,
    two: gix::ObjectId,
) -> Result<Option<gix::ObjectId>, Box<dyn std::error::Error + Send + Sync>> {
    let error = match git.merge_base(one, two) {
        Ok(id) => return Ok(Some(id.detach())),
        Err(error) => error,
    };
    let mut ancestors = std::collections::HashSet::new();
    for info in git.rev_walk([one]).all()? {
        ancestors.insert(info?.id);
    }
    for info in git.rev_walk([two]).all()? {
        if ancestors.contains(&info?.id) {
            return Err(error.into());
        }
    }
    Ok(None)
}
