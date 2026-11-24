//! What deciding an entry read of the working tree, named so that a later
//! run can tell whether a change could alter that decision
//! (docs/architecture/performance/warm.md).
//!
//! A read is named `<directory>/<key>`, the directory's `/`-separated path
//! (empty at the root) and the key of a name looked up in it: the name in
//! ASCII lower case without one `.json`, as a lookup matches names. A whole
//! directory listed is named `<directory>/`. A change at a path touches its
//! own name and its directory's.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// The read a resolution that followed a symlink in the working tree
/// records. A symlink may lead anywhere, so such a decision may change
/// with any change.
pub const ANY: &str = "*";

/// Where a resolver records the reads of the working tree it makes, until
/// the caller takes them.
pub type Reads = Rc<RefCell<Vec<String>>>;

/// The names a change at the `/`-separated `path` touches: its own and its
/// directory's.
pub fn changed(path: &str) -> [String; 2] {
    let (directory, _) = split(path);
    [named(path), read(directory, "")]
}

/// The name a lookup of the entry at the `/`-separated `path` reads.
pub fn named(path: &str) -> String {
    let (directory, name) = split(path);
    read(directory, &key(name))
}

/// Records the names a lookup of `name`, whose segment is `segment`, in
/// `directory` reads: every name it may match.
pub(crate) fn lookup(into: &Reads, directory: &Path, name: &[u8], segment: Option<&str>) {
    let directory = directory.to_string_lossy();
    let mut reads = into.borrow_mut();
    reads.push(read(&directory, &key(&String::from_utf8_lossy(name))));
    if let Some(segment) = segment {
        let folded = segment.to_ascii_lowercase();
        reads.push(read(&directory, &key(&folded)));
        reads.push(read(&directory, &folded));
    }
}

/// Records that `directory` was listed whole.
pub(crate) fn listed(into: &Reads, directory: &Path) {
    let read = read(&directory.to_string_lossy(), "");
    into.borrow_mut().push(read);
}

/// `path`'s directory and name; the directory is empty at the root.
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// The key of `name`: in ASCII lower case, without one `.json`.
fn key(name: &str) -> String {
    let folded = name.to_ascii_lowercase();
    match folded.strip_suffix(".json") {
        Some(stem) => stem.to_owned(),
        None => folded,
    }
}

fn read(directory: &str, key: &str) -> String {
    format!("{directory}/{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_touches_every_name_a_lookup_that_could_match_it_reads_and_no_other() {
        let reads = Reads::default();
        lookup(&reads, Path::new("topics"), b"Foo", Some("Foo"));
        let looked: Vec<String> = reads.borrow().clone();
        for (path, touched) in [
            ("topics/foo.json", true),
            ("topics/FOO", true),
            ("topics/foo.json.json", false),
            ("topics/food.json", false),
            ("other/foo.json", false),
        ] {
            let hit = changed(path).iter().any(|name| looked.contains(name));
            assert_eq!(hit, touched, "{path}");
        }
    }

    #[test]
    fn a_change_at_the_root_touches_the_roots_listing_and_one_in_a_directory_does_not() {
        let reads = Reads::default();
        listed(&reads, Path::new(""));
        let looked: Vec<String> = reads.borrow().clone();
        assert!(changed("a.json").iter().any(|name| looked.contains(name)));
        assert!(!changed("d/a.json").iter().any(|name| looked.contains(name)));
    }
}
