//! What each walk listed in each directory, kept so that a later walk reads
//! the listing of a directory whose stat has not changed in place of
//! listing it again (docs/architecture/performance/warm.md).

use board_model::Stat;
use board_tree::{Directories, DirectoryListing, Item, ItemType};
use rusqlite::params;

use crate::open::IndexError;
use crate::store::Writer;

impl Writer {
    /// What the last walk listed in each directory whose path is UTF-8. A
    /// row that cannot be read is left out, so its directory is listed
    /// again.
    pub fn directories(&self) -> Result<Directories, IndexError> {
        let mut statement = self.connection().prepare_cached(
            "SELECT path, size, modified_ns, changed_ns, inode, mode, listed_ns, items
             FROM main.directories",
        )?;
        let rows = statement.query_map([], |row| {
            let path: String = row.get(0)?;
            let stat = Stat {
                size: i64::cast_unsigned(row.get(1)?),
                modified_ns: row.get(2)?,
                changed_ns: row.get(3)?,
                inode: i64::cast_unsigned(row.get(4)?),
                mode: row.get(5)?,
            };
            let listed_ns: i64 = row.get(6)?;
            let items = decode_items(row.get_ref(7)?.as_bytes()?);
            Ok((path, stat, listed_ns, items))
        })?;
        let mut directories = Directories::new();
        for row in rows {
            let (path, stat, listed_ns, items) = row?;
            let Some(items) = items else { continue };
            let listed = DirectoryListing {
                stat,
                listed_ns,
                items,
            };
            directories.insert(path.into(), listed);
        }
        Ok(directories)
    }

    /// Stores `now`, what a walk listed in each directory, in place of
    /// `earlier`, what the index held, writing only the rows that differ.
    /// A listing a later walk could not trust
    /// ([`DirectoryListing::lasts`]) is not stored, nor is a directory
    /// whose path is not UTF-8, so each walk lists them; a row a walk did
    /// not reach is deleted.
    pub fn sync_directories(
        &self,
        earlier: &Directories,
        now: &Directories,
    ) -> Result<(), IndexError> {
        for (path, listed) in now {
            let Some(text) = path.to_str() else { continue };
            if earlier.get(path) == Some(listed) || !listed.lasts() {
                continue;
            }
            let stat = &listed.stat;
            self.connection()
                .prepare_cached(
                    "INSERT OR REPLACE INTO main.directories
                     (path, size, modified_ns, changed_ns, inode, mode, listed_ns, items)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )?
                .execute(params![
                    text,
                    stat.size.cast_signed(),
                    stat.modified_ns,
                    stat.changed_ns,
                    stat.inode.cast_signed(),
                    stat.mode,
                    listed.listed_ns,
                    encode_items(&listed.items),
                ])?;
        }
        for path in earlier.keys().filter(|path| !now.contains_key(*path)) {
            self.connection()
                .prepare_cached("DELETE FROM main.directories WHERE path = ?1")?
                .execute([path.to_string_lossy()])?;
        }
        Ok(())
    }
}

/// Each item's type, as one byte, and its name, ended by a zero byte, which
/// no file name holds.
fn encode_items(items: &[Item]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for item in items {
        bytes.push(match item.file_type {
            Some(ItemType::Directory) => b'd',
            Some(ItemType::Symlink) => b'l',
            Some(ItemType::File) => b'f',
            None => b'?',
        });
        bytes.extend_from_slice(&item.name);
        bytes.push(0);
    }
    bytes
}

/// The items `encode_items` wrote, or `None` when `bytes` are not such.
fn decode_items(bytes: &[u8]) -> Option<Vec<Item>> {
    let mut items = Vec::new();
    let mut rest = bytes;
    while let Some((&kind, after)) = rest.split_first() {
        let file_type = match kind {
            b'd' => Some(ItemType::Directory),
            b'l' => Some(ItemType::Symlink),
            b'f' => Some(ItemType::File),
            b'?' => None,
            _ => return None,
        };
        let end = after.iter().position(|&byte| byte == 0)?;
        items.push(Item {
            name: after[..end].to_vec(),
            file_type,
        });
        rest = &after[end + 1..];
    }
    Some(items)
}
