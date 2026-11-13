//! Changing git's index for the paths a commit wrote or removed.

use std::io::Write;

/// Points the index entry for `path` at `blob`, adding it when new. Its
/// stat is cleared, so git hashes the file again before trusting it.
pub(super) fn stage_entry(index: &mut gix::index::File, path: &str, blob: gix::ObjectId) {
    use gix::bstr::ByteSlice;
    index.remove_tree();
    if let Ok(at) = index.entry_index_by_path(path.as_bytes().as_bstr()) {
        let entry = &mut index.entries_mut()[at];
        entry.id = blob;
        entry.stat = gix::index::entry::Stat::default();
    } else {
        index.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            blob,
            gix::index::entry::Flags::empty(),
            gix::index::entry::Mode::FILE,
            path.as_bytes().as_bstr(),
        );
        index.sort_entries();
    }
}

/// Removes the index entry for `path`.
pub(super) fn unstage_entry(index: &mut gix::index::File, path: &str) {
    index.remove_tree();
    index.remove_entries(|_, at, _| at == path.as_bytes());
}

/// Writes `index` to `out`. The index writer makes about fifteen small
/// writes per entry, which on a file are system calls, so it writes into a
/// buffer that reaches `out` in large blocks.
pub(super) fn write_index(
    index: &gix::index::File,
    out: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let mut buffered = std::io::BufWriter::with_capacity(1 << 20, out);
    index
        .write_to(&mut buffered, gix::index::write::Options::default())
        .map_err(std::io::Error::other)?;
    buffered.flush()
}
