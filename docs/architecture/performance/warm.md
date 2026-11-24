# Warm runs

This file holds detail for docs/architecture.md. It says what the index
keeps so that a run on an unchanged board neither lists unchanged
directories nor decides the working tree again, and the states of each.

## What is kept

- `directories`: one row per directory the last writing walk listed, by its
  path in the working tree. It holds the directory's stat (size, times,
  inode, mode) read just before the listing, when that walk began, and the
  items `read_dir` gave, each a name and a type.
- `decided` rows of the working tree, under a key of the entries and the
  directories the walk saw, with the other inputs below. Each row keeps
  its `change`, `committed` or `uncommitted`. A mounted source's rows keep
  no `change`, and read as `committed`
  ([decisions.md](decisions.md)).

## The directory listings

- A walk reads each directory's stat first. When the index holds a listing
  of that path with the same stat, the walk uses its items in place of
  listing the directory.
- A listing is used only when it lasts: the directory's mtime falls in an
  earlier whole second than the second the listing's walk began. A
  directory changed in that second may have changed after it was listed.
- A listing is used only when its walk began no later than the current
  walk. A clock that steps back makes every directory listed again.
- The items are what the file system lists. Ignore rules are applied to them
  on every walk, so a changed `.gitignore` or exclude file changes the
  entries without changing a listing.
- Each file's own stat is read on every walk, so an in-place write to a file
  is seen whether or not its directory was listed.
- A walk that writes the index stores the listings that last and differ from
  the stored ones, and deletes the rows of directories it did not reach. A
  walk that writes nothing stores no listing, so an unchanged run takes no
  write lock.
- A directory whose path is not UTF-8 is not stored and is listed on every
  walk. A stored row whose items cannot be read is ignored, and its
  directory is listed.
- A warm walk of an unchanged board lists no directory.

## The working tree's decision

- The key hashes:
  - the build, as for a mounted source. A build that does not name itself
    keeps nothing;
  - the board root's path;
  - the tree `HEAD` names, which says whether each entry is committed;
  - the moved prefixes a resolution tries;
  - for each entry the walk listed: its path, logical path, blob, status,
    size, and a symlink's blob and target;
  - each directory and submodule the walk saw, with which it is. A link
    can name a directory that holds no entry, such as an empty one;
  - the working tree's mount probes, as a mounted source's key holds them.
- Each entry's blob names its record, schema or file, so a changed record or
  schema changes the key.
- A run that is not scoped computes the key after it walks the mounts. When
  the index holds rows under it and the command reads kept rows (`query`,
  `call`, `enable` and `check`), the run reads them in place of deciding,
  and checks no record against its schema.
- Otherwise the run reads `HEAD`'s tree, checks the records that need it,
  and decides every entry. It keeps the rows when it decided the whole
  tree, nothing failed to read, no two entries share a path, no entry is
  `board::repo::unreadable`, and every mount its resolutions crossed is one
  the key's probes hold.
- The `decided` work count ([timing.md](timing.md)) is the entries a run
  decided. A warm `query` or
  `check` of an unchanged board decides none.

## Lifecycle

### Directory listing

| From | Event | To | Owner |
| --- | --- | --- | --- |
| absent | a writing walk lists the directory and the listing lasts | stored | board-index `sync_directories` |
| absent | the listing does not last, or the path is not UTF-8 | absent; the next walk lists it | board-tree walk |
| stored | a walk reads the same stat, and the listing lasts and began no later | used; nothing is written | board-tree walk |
| stored | the directory's stat differs | listed again; replaced if the walk writes and the listing lasts | board-tree walk |
| stored | the clock reads earlier than the stored walk's start | listed again | board-tree walk |
| stored | a writing walk does not reach the directory | deleted | board-index `sync_directories` |
| any | the index format changes | absent, in a new index file | board-index open |

### Kept working-tree rows

| From | Event | To | Owner |
| --- | --- | --- | --- |
| absent | a run decides the whole tree and the rows may be kept | kept under the key, in the run's transaction | board-ops snapshot |
| kept | a run computes the same key and reads kept rows | read; reached by the run | board-ops snapshot |
| kept | an entry, `HEAD`'s tree, a moved prefix, a probe or the build changes | not read; the run decides and keeps rows under the new key | board-ops snapshot |
| kept | `refs`, or a scoped run | not read; the scoped run keeps nothing | board-ops snapshot |
| kept | a prune in a run that did not reach the key | deleted | board-index prune |

## Crash points

- A run that dies during the walk has written nothing. The next run lists
  the directories whose stat changed and decides again under the new key.
- The listings, the working-tree rows, the facts and the kept rows commit
  in one SQLite transaction. A run that dies or fills the disk while
  writing rolls all of them back, and the next run finds the old state.
- A directory changed while it is listed has a stat, read before the
  listing, that differs from its stat on the next walk, so it is listed
  again.
- Two runs that write at once take SQLite's write lock in turn. Each stores
  what it found. A listing that does not match the directory's stat is not
  used, and a key that does not match the tree is not read.
- A prune between a run's write and its read of kept rows ends the read
  with `IndexError::Pruned`, and the run runs again.

## Invariants

- A warm run's output is byte-identical to a cold run's for the same tree,
  `HEAD`, mounts and build. Tests check this after each change of an entry,
  `HEAD`, an ignore rule, git's index, a schema, a mount, a prune, a
  file dated ahead of the clock, and an empty directory made, removed or
  hidden by `.git/info/exclude`.
- A run on an unchanged board writes nothing and takes no write lock.
- Each walk reads the stat of every directory and every file.
- A listing whose directory changed in or after the second its walk began
  is never used.
- Kept working-tree rows are read only under a key that holds every entry
  and every directory the walk saw.

## Limits

- A listing is trusted when the directory's stat is unchanged. On a file
  system that keeps mtimes to 2 seconds, such as FAT32, or a network file
  system whose server clock lags the walk's by more than a second, an item
  added after a listing can leave the stat unchanged, and the walk uses
  the stale listing. The racy-clean rule for files has the same limit.
