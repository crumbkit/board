# Warm runs

This file holds detail for docs/architecture.md. It says what the index
keeps so that a run neither lists unchanged directories, opens unchanged
mounted repositories, nor decides working-tree entries a change cannot
alter, and the states of each.

## What is kept

- `directories`: one row per directory the last writing walk listed, by its
  path in the working tree. It holds the directory's stat (size, times,
  inode, mode) read just before the listing, when that walk began, and the
  items `read_dir` gave, each a name and a type.
- `probes`: one row per mounted repository a run opened, under a digest of
  its stamp and pinned commit. It holds the repository's path, the commit's
  root tree, the common git directory and the stored `origin`.
- `slots`: one row per build and board root. It holds the key of the
  working tree whose rows the slot keeps, the frame, `HEAD`'s tree, the
  directories and submodules the walk saw, and each working-tree mount
  pointer's probe digest.
- `decided` rows of the working tree, under the slot. Each row keeps its
  `change`, `committed` or `uncommitted`. A mounted source's rows keep no
  `change`, and read as `committed` ([decisions.md](decisions.md)).
- `inputs`: one row per entry of the slot. It holds a digest of the entry
  as deciding reads it, and each name its decision read.

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

## The working tree's facts

- Before the walk, the load scans the stored working-tree rows, the blobs
  table and the links table once each, in key order. It sorts the rows'
  blobs and matches them against the blobs table in that order, so it
  hashes no key and seeks no blob. It keeps the fact of each row's blob in
  the role its path gives it, and that blob's links.
- The load copies only what it must out of each row. A row's paths share
  one string, and a logical path that the file's own path gives is not
  copied. The walk and the sync find a row by its path in path order.
- The walk reads a record beside its directory, to learn whether it hides
  the directory, only when its blob has no fact or its fact says it may
  hold a pointer. A file whose stat changed is hashed first, so the blob
  checked is the file's current one.
- A blob the load did not find is looked up on its own. A warm run of an
  unchanged board looks up no blob and reads no record.

## Mount probes

- A mounted repository's stamp holds:
  - its path, canonical path, `.git` file or directory, git directory and
    common directory. A directory's inode and device are stamped, without
    its times, which every commit changes;
  - its `config`, `config.worktree` and `commondir`, and each file their
    `include` and `includeIf` sections name, under any condition;
  - `objects/info/alternates`, the `objects/pack` directory's stat and the
    pinned commit's loose object;
  - the global and system config files the board's own repository read,
    the files they include, and each config value that came from no file.
- A file's stat is its size, times, inode, device and mode, or that it is
  absent.
- When the index holds a probe under the stamp, the walk uses it and does
  not open the repository. Otherwise the walk opens the repository, finds
  the commit, and keeps a probe under the stamp.
- A failed open or a lost commit keeps no probe, so the next run opens the
  repository again.
- A stamp that cannot be read, other than for a missing file, keeps no probe.
- A build that does not name itself keeps no probe.
- `pointers` rows hold each pointer's `repo`, `follow` and commit, so a run
  reads a source's mounts from the index without opening it.
- The `repositories` work count ([timing.md](../timing.md)) is the repositories
  a run opened. A warm query of an unchanged board opens none.

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
- The frame is a digest of the moved prefixes. A different frame makes
  every entry decided.
- A run that is not scoped computes the key after it walks the mounts. When
  the slot holds that key and the command reads kept rows (`query`, `call`,
  `enable` and `check`), the run reads the rows in place of deciding, and
  checks no record against its schema.
- Otherwise the run reads `HEAD`'s tree and decides again only the entries
  below. It checks against its schema only each record it decides. It decides every entry when the
  slot holds nothing or another frame, two entries share a path, or more
  than one entry in 20 would be decided.
- The run keeps the rows when nothing failed to read, no two entries share a
  path, no entry is `board::repo::unreadable`, and every mount its
  resolutions crossed is one the key's probes hold. Otherwise it decides
  every entry and keeps none. The slot keeps the rows, inputs and key of
  the earlier tree it holds, and the next run compares the walk with them.
- A run that does not keep its rows records no reads.
- The `decided` work count ([timing.md](../timing.md)) is the entries a run
  decided. A warm `query` or `check` of an unchanged board decides none.

## Deciding again after a change

- Deciding an entry records each name it read in the working tree:
  - `<directory>/<key>` for a lookup, where the key is the name in ASCII
    lower case without one `.json`;
  - `<directory>/` for a directory listed whole;
  - `*` when a resolution followed a symlink, so any change decides it again;
  - `#remotes` for a repo record, which reads every repo record's `remote`;
  - its schema's path and each name on the way to it.
- A change at a path touches its own name and its directory's listing.
- The run touches the names of each entry whose digest changed, each entry
  added, each entry removed, each directory or submodule seen or no longer
  seen, and each mount pointer whose probe changed.
- The run decides again each entry that changed or was added, and each that
  read a touched name. A change to a schema touches the schema's path, so
  the records checked against it are decided again.
- `HEAD` moving changes no entry's name. The run reads the paths whose blobs
  differ between the old and new `HEAD` trees from the index's tree rows,
  and sets those rows' `change` without deciding them. When the index does
  not hold either tree, every entry is decided.
- The run keeps the rows it decided in place of those paths' rows and
  deletes the rows of removed entries. The order of rows read from the slot
  is the order of a cold decision.

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

### Mount probe

| From | Event | To | Owner |
| --- | --- | --- | --- |
| absent | a run opens the repository and finds the commit | kept under the stamp, in the run's transaction | board-tree walk_mounts |
| absent | the open or the commit lookup fails | absent; the next run opens it again | board-tree walk_mounts |
| kept | a run computes the same stamp | used; the repository is not opened | board-tree walk_mounts |
| kept | the stamp differs | not read; the run opens the repository and keeps a probe under the new stamp | board-tree walk_mounts |
| kept | a prune in a run that did not use it | deleted | board-index prune |
| any | the index format changes | absent, in a new index file | board-index open |

### Working-tree slot

| From | Event | To | Owner |
| --- | --- | --- | --- |
| absent | a run decides the whole tree and the rows may be kept | holds the key, rows, references and inputs | board-ops snapshot |
| holds key K | a run computes K | rows read, and for `refs` references; nothing written | board-ops snapshot |
| holds key K | a run computes K2, and at most one entry in 20 is decided | changed rows, references and inputs replaced; holds K2; a prune counted | board-ops snapshot |
| holds key K | a run computes K2, and the frame differs or more is decided | every row, reference and input replaced; holds K2; a prune counted | board-ops snapshot |
| holds key K | the new rows may not be kept | the slot keeps key K, its rows and its inputs; no prune counted | board-ops snapshot |
| holds key K | a scoped run | not read and not written | board-ops snapshot |
| any | a prune in a run that did not reach the slot | slot, rows, references and inputs deleted | board-index prune |

## Crash points

- A run that dies during the walk has written nothing. The next run lists
  the directories whose stat changed and decides again under the new key.
- The listings, the working-tree rows, the facts, the probes, the pointer
  lists, the slot with its rows, references and inputs, and the prune count commit in
  one SQLite transaction. A run that dies or fills the disk while
  writing rolls all of them back, and the next run finds the old state.
- A directory changed while it is listed has a stat, read before the
  listing, that differs from its stat on the next walk, so it is listed
  again.
- Two runs that write at once take SQLite's write lock in turn. Each stores
  what it found. A listing that does not match the directory's stat is not
  used, and a key that does not match the tree is not read.
- A prune between a run's write and its read of kept rows ends the read
  with `IndexError::Pruned`, and the run runs again. A rewrite of the slot
  counts as a prune, so a view filled from its earlier rows does the same.
- A repository that changes between its stamp and its open leaves a probe
  under the earlier stamp, which the next run does not match.

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
- Kept working-tree rows are read whole only when the slot holds a key that
  holds every entry and every directory the walk saw.
- The slot's key, rows and inputs change together, in one transaction.
- After a change, a row is read from the slot only when the frame is
  unchanged and its entry read no name the change touched.
- A run opens no mounted repository whose stamp is unchanged.

## Limits

- A listing is trusted when the directory's stat is unchanged. On a file
  system that keeps mtimes to 2 seconds, such as FAT32, or a network file
  system whose server clock lags the walk's by more than a second, an item
  added after a listing can leave the stat unchanged, and the walk uses
  the stale listing. The racy-clean rule for files has the same limit.
