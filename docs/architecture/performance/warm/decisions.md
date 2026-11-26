# Kept decisions

This file holds detail for docs/architecture.md. It says how the index keeps
each mounted source's decision so a warm query or check neither walks nor
decides it. The working tree's decision is kept as [index.md](index.md) says.

## What is kept

- `decided`: one row per entry of a source, as a query's views show it,
  under the source's key. The source is not kept: each run attaches the
  source that reached the key. A mounted source's rows keep no `change`,
  as every entry of a commit is committed. Its
  `detail` column holds, as JSON, what `check` reports beyond the row: a
  degraded entry's detail and the schema it names. The `decided` view
  leaves it out.
- `resolutions`: one row for each entry of a kept decision that makes
  references, holding them in order as a JSON array with where each
  resolved, under the same key, written in the same transaction as its
  rows and pruned with them. A location in the decided
  board itself keeps no board, so each run attaches the one that reached
  the key. `refs` reads them in place of deciding the source.
- `pointers`: the mount pointers in a mounted tree, under a key of the tree
  and the build, so a run finds a source's mounts without walking it.

## The key

A source's key hashes:

- the build: board-cli's version and commit, and for a build with
  uncommitted changes a hash of them. A build that does not name itself,
  such as one built outside a git checkout, keeps nothing;
- the source's route, the display path its entries are named under;
- its root tree and the repository it was read from;
- for each mount pointer in it: its path, `repo` and commit, and the
  repository it opened, or the code that says why it did not;
- for each mount it reaches, the same of that source, without its route.

A source with a file whose blob its repository lacks, such as one read
from a partial clone, is kept under a second key. That key hashes the
first and a stamp of the repository's objects: the stat of its pack
directory and of each directory of loose objects, taken before the walk
reads the source. A fetch or a loose write that brings a blob changes the
stamp, so the source is decided again. A run
stamps a source's objects only when the index does not hold its decision
under the first key.

A run opens a mounted repository and looks up its pinned commit only when
the repository's stamp changed ([index.md](index.md)), so a deleted repository
or a lost commit still changes the key. The repositories one source's mounts
name are opened in parallel, one thread each, before its mounts are read in
order.

## Who reads and who fills

- `query`, `call` and `enable` read kept rows in place and decide only the
  sources whose key the index does not hold.
- `check` reads, in path order, the kept rows of each source wholly in its
  scope. It walks and decides the other sources its scope reaches, and
  keeps what it decided, except for a source it decided only in part. A
  row that cannot be read fails the run with `board::index::failed`.
- `refs` reads the kept rows and references of each source whose key the
  index holds, and checks the blob of a target there against its kept row.
  It decides only the other sources, and keeps what it decided.
- A scoped run decides the working tree only in part, and keeps none of it.
- `ls` reads kept rows as `check` does: the kept rows of each mounted
  source wholly in its scope, walking only the directory it lists there.
  It walks and decides the other sources its scope reaches, and keeps
  nothing for a source it decided only in part. An `ls` of the root is
  unscoped, so it also reads the working tree's kept decision.
- A scope reaches a source when one of the source's routes lies inside the
  scope, or the scope lies inside one of its routes. A scoped `check` or
  `ls` walks any other source only when the index lists no
  pointers for it, to find its mounts.
- On a copy of the owner's board with 24 mounts, a warm `check /controls`
  takes about 38 ms. It took 59 ms when a scoped run walked every source.
  On the same copy, a warm `call plans.open --limit 20` takes about 47 ms,
  down from 96 ms, which meets the 50 ms target in budgets.md.

## What is never kept

These sources are decided on every run:

- one whose read or resolution failed with a git error, which may pass,
  such as a reference to a blob another repository lacks;
- one with a file whose blob its repository holds but could not read, as
  after an I/O error;
- one with a file whose blob its repository lacks, when that repository
  borrows objects through alternates, which the stamp does not cover;
- one holding a mount whose route is too long to name;
- one that reaches a commit through another repository than the one that
  commit was walked in;
- one whose resolution crossed a mount that no probe holds, such as a
  pointer in a schema file;
- one holding a pointer whose blob cannot be read;
- one with two names that are not UTF-8 and read as the same path.

## Order and pruning

- `entries` lists the kept rows by source, then path, then the rest: the
  rows of sources not kept. Whether a source is kept
  depends on the source, not on the cache, so a query without `ORDER BY`
  answers alike warm and cold.
- A prune deletes kept rows and pointer lists the run did not reach.
