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
and no record is written; when both inserted the same lines, one copy stays.
This covers a phone and an agent each appending to
the end of a daily note. Changes that overlap or touch in any other way, for
example an insertion next to lines the other side deleted, are a conflict.
A remote insertion that does not end in a newline is also a conflict, since
the two insertions would join into one line. A text file that starts with frontmatter (a first line `---`, after an optional byte order mark, and a closing line `---` or `...`) is
split at its closing `---` line: the body is merged by line as above, and the
frontmatter is taken whole, from the side that changed it, or the remote's
when both changed it differently, which also writes a conflict record (the
body is still merged by line). A frontmatter is never a union of lines, so
two machines that add the same field cannot make a duplicate key. A JSON
record (which includes
every mount pointer), a `.gitattributes` file and a file that is not text
are never merged by line. A path both sides added with different content is merged by line from an
empty ancestor, so both texts stay, unless either side starts with
frontmatter: then the remote's file is in the merge and a record is written. Otherwise, and when both edited one of those or
added different content, the remote's content is in the merge. When one side edited a file and the other deleted it, the edit
is. When one side holds a file where the other holds a directory, the remote's
is. The other version stays in history.

The conflict records, and the lifecycle of the merge, are in [conflicts](merging/conflicts.md).

Two mount pointers that differ only in `commit` are settled without a record unless their ancestry cannot be read, as below.
The pin that descends from the other is kept, read in the mounted repository.
When neither descends, the remote's is kept. When a pinned commit is missing
there, so that ancestry cannot be read, the remote's pin is kept, because the
machine that pushed it had its commit and the local pin may be an ancestor of
it, and a `board/conflict/v1` record names both pins. The report says so
(`basis` is `unknown`). A pin that was pushed therefore never moves back when
another machine merges it.

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
