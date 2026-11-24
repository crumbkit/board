//! The keys a cache keeps mounted sources' decisions and pointer lists
//! under (docs/architecture/performance/decisions.md).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use board_model::{Code, Digest, ObjectId, Source};

use super::{Crossing, Reached};
use crate::git::Repository;

/// A mount pointer as probed: what it names, and the repository it opened
/// or why it could not be read.
#[derive(Debug)]
pub(super) struct Probe {
    pub(super) from: Source,
    pub(super) path: String,
    pub(super) repo: String,
    pub(super) commit: ObjectId,
    pub(super) landed: Result<PathBuf, Code>,
}

/// The key of the pointers the tree `root` holds, as `build` lists them.
pub(super) fn scan(build: &str, root: ObjectId) -> Digest {
    hashed(&[b"pointers", build.as_bytes(), root.to_string().as_bytes()])
}

impl<F> Reached<F> {
    /// Sets each source's key: the code that decides, its route, and what
    /// deciding it reads wherever it is mounted.
    pub(super) fn key(&mut self, build: &str) {
        let mut closures = HashMap::new();
        let keys: Vec<Option<Digest>> = (self.sources.iter())
            .map(|mounted| {
                let closure = self.closure(mounted.commit, &mut closures)?;
                let route = mounted.route.as_str().as_bytes();
                Some(hashed(&[
                    b"decision",
                    build.as_bytes(),
                    route,
                    closure.hex().as_bytes(),
                ]))
            })
            .collect();
        let crossings: Vec<HashSet<Crossing>> = (self.sources.iter())
            .map(|mounted| self.crossings(mounted.commit))
            .collect();
        for ((mounted, key), crossings) in self.sources.iter_mut().zip(keys).zip(crossings) {
            mounted.key = key;
            mounted.crossings = crossings;
        }
    }

    /// Each mount the probes of `commit`'s source and of every source it
    /// opens hold.
    fn crossings(&self, commit: ObjectId) -> HashSet<Crossing> {
        let Some(mounted) = (self.sources.iter()).find(|mounted| mounted.commit == commit) else {
            return HashSet::new();
        };
        let start = (Source::Commit(commit), mounted.repository.clone());
        self.crossings_from(start)
    }

    /// Each mount the probes of the source `from`, read in the repository
    /// at the path beside it, and of every source it opens hold.
    fn crossings_from(&self, from: (Source, PathBuf)) -> HashSet<Crossing> {
        let mut held = HashSet::new();
        let mut pending = vec![from];
        let mut seen = HashSet::new();
        while let Some((from, repository)) = pending.pop() {
            if !seen.insert(from.clone()) {
                continue;
            }
            for probe in self.probes.iter().filter(|probe| probe.from == from) {
                held.insert((repository.clone(), probe.repo.clone(), probe.commit));
                let child = (self.sources.iter()).find(|mounted| mounted.commit == probe.commit);
                if let (Ok(_), Some(child)) = (&probe.landed, child) {
                    let path = child.repository.clone();
                    pending.push((Source::Commit(probe.commit), path));
                }
            }
        }
        held
    }

    /// The key of the working tree's decision, when deciding it reads only
    /// what the key holds: `build`, the code that decides, then `fields`,
    /// what the caller knows it reads, then what each of the working tree's
    /// mounts names and reaches, as a mounted source's key holds it. Also
    /// each mount its decision may cross because the key holds its probe.
    /// `None` as for a mounted source, or when `build` is empty.
    pub fn worktree_key(
        &self,
        build: &str,
        root: &Repository,
        fields: &[&[u8]],
    ) -> Option<(Digest, HashSet<Crossing>)> {
        if build.is_empty() {
            return None;
        }
        let mut closures = HashMap::new();
        let mut all: Vec<Vec<u8>> = vec![b"worktree".to_vec(), build.as_bytes().to_vec()];
        all.push(root.path().as_os_str().as_encoded_bytes().to_vec());
        all.extend(fields.iter().map(|field| field.to_vec()));
        all.extend(self.probed(&Source::WorkingTree, &mut closures)?);
        let start = (Source::WorkingTree, root.path().to_path_buf());
        Some((hashed(&all), self.crossings_from(start)))
    }

    /// What deciding the source of `commit` reads, hashed: its root tree,
    /// the repository it was read from, and for each of its mounts what
    /// the pointer names and what it reached, or why not and, for a cycle,
    /// the route of the board it repeats. `None` when one of its
    /// pointers could not be read, when one of its mounts is not walked, or
    /// when one reaches a commit through a repository other than the one
    /// that commit was walked in.
    fn closure(
        &self,
        commit: ObjectId,
        closures: &mut HashMap<ObjectId, Option<Digest>>,
    ) -> Option<Digest> {
        if let Some(known) = closures.get(&commit) {
            return *known;
        }
        // Commits cannot form a cycle; were one found, it stays unkeyed.
        closures.insert(commit, None);
        let mounted = self
            .sources
            .iter()
            .find(|mounted| mounted.commit == commit)?;
        let from = Source::Commit(commit);
        let mut fields = vec![
            mounted.root.to_string().into_bytes(),
            mounted.repository.as_os_str().as_encoded_bytes().to_vec(),
        ];
        fields.extend(self.probed(&from, closures)?);
        let closure = hashed(&fields);
        closures.insert(commit, Some(closure));
        Some(closure)
    }
}

impl<F> Reached<F> {
    /// What each mount pointer of the source `from` names and what it
    /// reached, or why not and, for a cycle, the route of the board it
    /// repeats. `None` when one of its pointers could not be read, when one
    /// of its mounts is not walked, or when one reaches a commit through a
    /// repository other than the one that commit was walked in.
    fn probed(
        &self,
        from: &Source,
        closures: &mut HashMap<ObjectId, Option<Digest>>,
    ) -> Option<Vec<Vec<u8>>> {
        if self.unread.contains(from) {
            return None;
        }
        let mut probes: Vec<&Probe> = (self.probes.iter())
            .filter(|probe| probe.from == *from)
            .collect();
        probes.sort_by(|a, b| a.path.cmp(&b.path));
        let mut fields = Vec::new();
        for probe in probes {
            fields.push(probe.path.clone().into_bytes());
            fields.push(probe.repo.clone().into_bytes());
            fields.push(probe.commit.to_string().into_bytes());
            match &probe.landed {
                Err(Code::PathMalformed) => return None,
                Err(code) => {
                    fields.extend([b"refused".to_vec(), code.as_str().into()]);
                    let at = (from.clone(), probe.path.clone());
                    if let Some(repeats) = self.cycles.get(&at) {
                        fields.push(repeats.as_str().into());
                    }
                }
                Ok(opened) => {
                    let child = self
                        .sources
                        .iter()
                        .find(|child| child.commit == probe.commit)?;
                    if child.repository != *opened {
                        return None;
                    }
                    let closure = self.closure(probe.commit, closures)?;
                    fields.push(b"opened".to_vec());
                    fields.push(opened.as_os_str().as_encoded_bytes().to_vec());
                    fields.push(closure.hex().into_bytes());
                }
            }
        }
        Some(fields)
    }
}

/// The digest of `fields`, each preceded by its length so that no two
/// lists of fields give the same bytes.
pub(super) fn hashed<T: AsRef<[u8]>>(fields: &[T]) -> Digest {
    let mut bytes = Vec::new();
    for field in fields {
        let field = field.as_ref();
        bytes.extend(field.len().to_le_bytes());
        bytes.extend_from_slice(field);
    }
    Digest::of(&bytes)
}

#[cfg(test)]
mod tests;
