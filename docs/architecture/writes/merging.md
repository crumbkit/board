# Merging a sync

A sync merges the fetched commit into HEAD, reading only the paths whose
entries differ between the common ancestor and the two sides. HEAD already holding the fetched commit changes nothing, and HEAD behind it
moves to it. Otherwise the merge is computed in memory, and its commit is
written before any file is touched.

When one side changed a path, that change is taken, and so is a change both
made. When both edited a text file (any file that is not `.json` or
`.gitattributes`), a line-level three-way merge from the ancestor runs first,
and its result is taken with no conflict when no changed hunks overlap. Two
changes that only insert lines at the same point of the ancestor, and delete
nothing there, both stay, the remote's lines first and then the local ones,
and no record is written. This covers a phone and an agent each appending to
the end of a daily note. Changes that overlap or touch in any other way, for
example an insertion next to lines the other side deleted, are a conflict.
A remote insertion that does not end in a newline is also a conflict, since
the two insertions would join into one line. A JSON record (which includes
every mount pointer), a `.gitattributes` file and a file that is not text
are never merged by line. Otherwise, and when both edited one of those or
added different content, the remote's content is in the merge. When one side edited a file and the other deleted it, the edit
is. When one side holds a file where the other holds a directory, the remote's
is. The other version stays in history.

For each such path the merge commit also holds a `board/conflict/v1` record at
`conflicts/<name>.json`, where `<name>` is the hash of the path and of the
three blobs in dispute (the ancestor's and each side's, or `-` for none). The
same conflict has the same name, so a record already in the merge is kept as
it is, and a later conflict at the same path, over other content, has a record
of its own that leaves the earlier one alone. The record holds:

- `path` and `kind`, one of `both_changed`, `edit_delete` or `file_directory`.
- `kept`, `local` or `remote`, the side whose content is in the file.
- `base`, the ancestor's blob, or null when the ancestor had no file.
- `local` and `remote`, each with the `blob` (null when that side held no
  file), the `commit` that last changed the path on that side, and its `author`
  as `Name <email>`.

Entries under `conflicts/` are never the subject of a conflict. When both
sides hold different bytes at one `conflicts/` path, the remote's are kept,
and when one side changed or deleted a record and the other did not, the
change is taken; neither writes a record. Two clones that make the same
conflict, then merge each other's work, therefore stay at one record.

Two mount pointers that differ only in `commit` are settled without a record.
The pin that descends from the other is kept, read in the mounted repository.
When neither descends, the remote's is kept. When a pinned commit is missing
there, the local pin is kept, and the report says so.

The report holds `merged` (whether `new` is a merge commit made by this sync),
`conflicts` (each `path`, `kind` and `record`) and `pins` (each `path`, the
`local` and `remote` commit, the side `taken` and its `basis`). It also lists local records that fail a schema both sides changed, as
the remote's schema reads them (`failing`, paths). They are reported and not changed.

`last_change` credits `local` and `remote` in a record to the last commit on
that side's first-parent chain that changed the path, so a change that
arrived through a second parent is credited to the merge commit.

gix gives one merge base for two commits. When the histories hold more than one
best common ancestor (each clone merged the other's work, a criss-cross), the
merge is made against that one base, and a change that is in another base
may be taken as new. The merge does not look for the others.

A sync killed after some files were written leaves HEAD where it was. The next
sync computes the same merge and accepts the files already at their new content.
