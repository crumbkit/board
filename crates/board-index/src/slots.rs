//! The working tree's decision kept in a slot, one per build and board
//! root, with what a later run needs to decide again only what a change
//! could alter (docs/architecture/performance/warm/index.md). board-ops makes the
//! keys and the reads; this crate stores them and never reads into them.

use board_model::{Change, Digest, ObjectId};
use rusqlite::{OptionalExtension, params};

use crate::open::IndexError;
use crate::schema::COUNTED;
use crate::store::Writer;

/// What a slot holds besides its rows and inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// The hex key of the tree its rows describe.
    pub key: String,
    /// The hex digest of what, when it differs, makes every entry decided.
    pub frame: String,
    /// The tree HEAD named.
    pub head: Option<ObjectId>,
    /// Each directory and submodule the walk saw.
    pub directories: Vec<String>,
    /// Each mount pointer's path and the hex digest of its probe.
    pub probes: Vec<(String, String)>,
}

/// One entry of a slot's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub path: String,
    /// The hex digest of the entry as deciding reads it.
    pub entry: String,
    /// Each name its decision read.
    pub reads: Vec<String>,
}

/// Each of `names` ended by a zero byte.
fn joined<'n>(names: impl IntoIterator<Item = &'n str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for name in names {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes
}

/// The names `bytes` holds, each ended by a zero byte.
fn split(bytes: &[u8]) -> Vec<String> {
    (bytes.split(|byte| *byte == 0))
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

impl Writer {
    /// What the slot `slot` holds, or `None` when the index keeps none.
    pub fn slot(&self, slot: &Digest) -> Result<Option<Slot>, IndexError> {
        type Row = (String, String, Option<String>, Vec<u8>, Vec<u8>);
        let row: Option<Row> = self
            .connection()
            .prepare_cached(
                "SELECT key, frame, head, directories, probes FROM main.slots WHERE slot = ?1",
            )?
            .query_row([slot.hex()], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .optional()?;
        Ok(row.map(|(key, frame, head, directories, probes)| {
            let probes = split(&probes);
            Slot {
                key,
                frame,
                head: head.and_then(|head| ObjectId::from_hex(&head)),
                directories: split(&directories),
                probes: (probes.as_chunks::<2>().0.iter())
                    .map(|[path, probe]| (path.clone(), probe.clone()))
                    .collect(),
            }
        }))
    }

    /// The inputs of the slot `slot`, by path.
    pub fn inputs(&self, slot: &Digest) -> Result<Vec<Input>, IndexError> {
        let rows = self
            .connection()
            .prepare_cached("SELECT path, entry, reads FROM main.inputs WHERE slot = ?1")?
            .query_map([slot.hex()], |row| {
                Ok(Input {
                    path: row.get(0)?,
                    entry: row.get(1)?,
                    reads: split(&row.get::<_, Vec<u8>>(2)?),
                })
            })?
            .collect::<Result<Vec<Input>, _>>()?;
        Ok(rows)
    }

    /// Sets what the slot `slot` holds. When it `replaced` what the slot
    /// held, it counts a prune, since a view filled from the slot's earlier
    /// rows no longer shows them.
    pub fn put_slot(&self, slot: &Digest, held: &Slot, replaced: bool) -> Result<(), IndexError> {
        let directories = joined(held.directories.iter().map(String::as_str));
        let probes =
            joined((held.probes.iter()).flat_map(|(path, probe)| [path.as_str(), probe.as_str()]));
        self.connection()
            .prepare_cached(
                "INSERT OR REPLACE INTO main.slots (slot, key, frame, head, directories, probes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?
            .execute(params![
                slot.hex(),
                held.key,
                held.frame,
                held.head.map(|head| head.to_string()),
                directories,
                probes
            ])?;
        if replaced {
            self.connection().execute(COUNTED, [])?;
        }
        Ok(())
    }

    /// Keeps `inputs` in the slot `slot`, replacing those of their paths.
    pub fn put_inputs<'i>(
        &self,
        slot: &Digest,
        inputs: impl IntoIterator<Item = &'i Input>,
    ) -> Result<(), IndexError> {
        let slot = slot.hex();
        let mut statement = self.connection().prepare_cached(
            "INSERT OR REPLACE INTO main.inputs (slot, path, entry, reads) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for input in inputs {
            let reads = joined(input.reads.iter().map(String::as_str));
            statement.execute(params![slot, input.path, input.entry, reads])?;
        }
        Ok(())
    }

    /// Deletes the rows and inputs of the slot `slot` at `paths`, or every
    /// one when `paths` is `None`.
    pub fn forget(&self, slot: &Digest, paths: Option<&[&str]>) -> Result<(), IndexError> {
        let slot = slot.hex();
        let connection = self.connection();
        let Some(paths) = paths else {
            connection.execute("DELETE FROM main.decided WHERE key = ?1", [&slot])?;
            connection.execute("DELETE FROM main.inputs WHERE slot = ?1", [&slot])?;
            return Ok(());
        };
        let mut rows =
            connection.prepare_cached("DELETE FROM main.decided WHERE key = ?1 AND path = ?2")?;
        let mut inputs =
            connection.prepare_cached("DELETE FROM main.inputs WHERE slot = ?1 AND path = ?2")?;
        for path in paths {
            rows.execute(params![slot, path])?;
            inputs.execute(params![slot, path])?;
        }
        Ok(())
    }

    /// Sets the `change` of the slot's row at `path`.
    pub fn set_change(&self, slot: &Digest, path: &str, change: Change) -> Result<(), IndexError> {
        self.connection()
            .prepare_cached("UPDATE main.decided SET change = ?3 WHERE key = ?1 AND path = ?2")?
            .execute(params![slot.hex(), path, change.as_str()])?;
        Ok(())
    }
}
