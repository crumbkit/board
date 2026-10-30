//! The keys a cache keeps mounted sources' decisions and pointer lists
//! under (docs/architecture/performance/decisions.md).

use std::collections::HashMap;
use std::path::PathBuf;

use board_model::{Code, Digest, ObjectId, Source};

use super::Reached;

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
        for (mounted, key) in self.sources.iter_mut().zip(keys) {
            mounted.key = key;
        }
    }

    /// What deciding the source of `commit` reads, hashed: its root tree,
    /// the repository it was read from, and for each of its mounts what
    /// the pointer names and what it reached. `None` when one of its mounts
    /// is not walked, or reaches a commit through a repository other than
    /// the one that commit was walked in.
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
        let mut probes: Vec<&Probe> = (self.probes.iter())
            .filter(|probe| probe.from == from)
            .collect();
        probes.sort_by(|a, b| a.path.cmp(&b.path));
        let mut fields = vec![
            mounted.root.to_string().into_bytes(),
            mounted
                .repository
                .path()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec(),
        ];
        for probe in probes {
            fields.push(probe.path.clone().into_bytes());
            fields.push(probe.repo.clone().into_bytes());
            fields.push(probe.commit.to_string().into_bytes());
            match &probe.landed {
                Err(Code::PathMalformed) => return None,
                Err(code) => fields.extend([b"refused".to_vec(), code.as_str().into()]),
                Ok(opened) => {
                    let child = self
                        .sources
                        .iter()
                        .find(|child| child.commit == probe.commit)?;
                    if child.repository.path() != opened {
                        return None;
                    }
                    let closure = self.closure(probe.commit, closures)?;
                    fields.push(b"opened".to_vec());
                    fields.push(opened.as_os_str().as_encoded_bytes().to_vec());
                    fields.push(closure.hex().into_bytes());
                }
            }
        }
        let closure = hashed(&fields);
        closures.insert(commit, Some(closure));
        Some(closure)
    }
}

/// The digest of `fields`, each preceded by its length so that no two
/// lists of fields give the same bytes.
fn hashed<T: AsRef<[u8]>>(fields: &[T]) -> Digest {
    let mut bytes = Vec::new();
    for field in fields {
        let field = field.as_ref();
        bytes.extend(field.len().to_le_bytes());
        bytes.extend_from_slice(field);
    }
    Digest::of(&bytes)
}
