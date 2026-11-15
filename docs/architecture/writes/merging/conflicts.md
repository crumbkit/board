# Conflict records and the merge lifecycle

For each such path the merge commit also holds a `board/conflict/v1` record at
`conflicts/<name>.json`, where `<name>` is the hash of the path and of the
three blobs in dispute (the ancestor's and each side's, or `-` for none),
with the two sides' ids in sorted order, so two clones that merge the same
pair with the sides swapped make one name. The
same conflict has the same name, so a record already in the merge is kept as
it is, and a later conflict at the same path, over other content, has a record
of its own that leaves the earlier one alone. The record holds:

- `path` and `kind`, one of `both_changed`, `edit_delete` or `file_directory`.
- `kept`, `local` or `remote`, the side whose content is in the file.
- `base`, the ancestor's blob, or null when the ancestor had no file.
- `local` and `remote`, each with the `blob` (null when that side held no
  file), the `commit` that last changed the path on that side, and its `author`
  as `Name <email>`.

After the merge tree is built, `kept` in each record under `conflicts/` is
set to the side whose blob the merged tree holds at the record's path. This
matters when two clones merged each other's conflict, so the record that
survived came from the other clone's point of view. A record whose path holds
neither side's blob, for example after a frontmatter merge, keeps the `kept`
it was written with.

Entries under `conflicts/` are never the subject of a conflict. When both
sides hold different bytes at one `conflicts/` path, the remote's are kept,
and when one side changed or deleted a record and the other did not, the
change is taken; neither writes a record. Two clones that make the same
conflict, then merge each other's work, therefore stay at one record.

## Lifecycle

| From | Event | To | Owner |
| --- | --- | --- | --- |
| merging a pin | remote pin descends from local | remote pin taken | sync |
| merging a pin | local pin descends from remote | local pin taken | sync |
| merging a pin | neither descends | remote pin taken | sync |
| merging a pin | either commit missing from the child | remote pin taken, conflict record | sync |
| merging a Markdown record | only one side changed the frontmatter | that side's frontmatter, body line-merged | sync |
| merging a Markdown record | both sides changed the frontmatter differently | remote frontmatter, body line-merged, conflict record | sync |
| merging any file | both sides added the path with different content | remote file, conflict record | sync |
| merge tree built | a conflict record's path holds one side's blob | the record's `kept` names that side | sync |
| checking | git state reports a merge, rebase, cherry-pick or revert | refused (`board::sync::merging`) | a person finishes or aborts it |
| note or task write | sync refused `merging` | write fails with `board::sync::merging` | a person finishes or aborts the operation, then writes again |

Every decision above is made while the merge tree is built in memory, before
integrate writes a file, so the clone is left as [merging.md](../merging.md) describes it after a kill.
