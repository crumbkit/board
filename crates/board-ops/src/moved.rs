//! Globs under a moved prefix (docs/plugins/calling/commands.md, Renamed
//! plugins).
//! A reference resolves under the other prefix in board-tree; a glob is
//! matched by a plugin's own query, so it is swapped here.

use board_model::{LogicalPath, Source, swapped};

use crate::Board;

/// The characters that make a segment of a `SQLite` GLOB pattern a pattern.
const GLOB: [char; 3] = ['*', '?', '['];

impl Board {
    /// `pattern`, a glob by logical path such as `/controls/board/*`, with
    /// the moved prefix that begins its literal segments swapped for the
    /// other prefix, at the board's root or at a mounted board's root, as a
    /// reference is swapped; `None` when no moved prefix begins them there.
    /// A caller matches it only when `pattern` matches nothing, as a
    /// reference is resolved under the other prefix only when it names
    /// nothing.
    pub fn moved_glob(&self, pattern: &str) -> Option<String> {
        let segments: Vec<&str> = pattern.strip_prefix('/')?.split('/').collect();
        let literal = (segments.iter())
            .take_while(|segment| !segment.contains(GLOB))
            .count();
        let moved = self.repositories().moved();
        (0..literal).find_map(|at| {
            if at > 0 && !self.mounted_root(&segments[..at]) {
                return None;
            }
            let other = swapped(moved, &segments[at..literal])?;
            let before = segments[..at].iter().map(|segment| (*segment).to_owned());
            let after = segments[literal..]
                .iter()
                .map(|segment| (*segment).to_owned());
            let joined: Vec<String> = before.chain(other).chain(after).collect();
            Some(format!("/{}", joined.join("/")))
        })
    }

    /// Whether `segments` lead to the root of a mounted board.
    fn mounted_root(&self, segments: &[&str]) -> bool {
        let Ok(path) = format!("/{}", segments.join("/")).parse::<LogicalPath>() else {
            return false;
        };
        (self.repositories().resolve(&Source::WorkingTree, &path)).is_ok_and(|location| {
            location.path.is_empty() && location.board.source != Source::WorkingTree
        })
    }
}

#[cfg(test)]
mod tests;
