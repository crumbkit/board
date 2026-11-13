# Merging a sync

A sync merges the fetched commit into HEAD, reading only the paths whose
entries differ between the common ancestor and the two sides. HEAD already holding the fetched commit changes nothing, and HEAD behind it
moves to it. Otherwise the merge is computed in memory, and its commit is
written before any file is touched.

When one side changed a path, that change is taken, and so is a change both
made. When both edited a text file (any file that is not `.json`), a line-level
three-way merge from the ancestor runs first, and its result is taken with no
conflict when no changed hunks overlap. Otherwise, and when both edited a JSON
record or added different content, the remote's content is in the merge. When one side edited a file and the other deleted it, the edit
is. When one side holds a file where the other holds a directory, the remote's
is. The other version stays in history.

For each such path the merge commit also holds a `board/conflict/v1` record at
`conflicts/<name>.json`, where `<name>` is twelve hex digits of the path's
hash, so a later conflict at the same path replaces it. The record holds:

- `path` and `kind`, one of `both_changed`, `edit_delete` or `file_directory`.
- `kept`, `local` or `remote`, the side whose content is in the file.
- `base`, the ancestor's blob, or null when the ancestor had no file.
- `local` and `remote`, each with the `blob` (null when that side held no
  file), the `commit` that last changed the path on that side, and its `author`
  as `Name <email>`.

Two mount pointers that differ only in `commit` are settled without a record.
The pin that descends from the other is kept, read in the mounted repository.
When neither descends, the remote's is kept. When a pinned commit is missing
there, the local pin is kept, and the report says so.

The report holds `merged` (whether `new` is a merge commit made by this sync),
`conflicts` (each `path`, `kind` and `record`) and `pins` (each `path`, the
`local` and `remote` commit, the side `taken` and its `basis`). It also lists local records that fail a schema both sides changed, as
the remote's schema reads them (`failing`, paths). They are reported and not changed.

A sync killed after some files were written leaves HEAD where it was. The next
sync computes the same merge and accepts the files already at their new content.
