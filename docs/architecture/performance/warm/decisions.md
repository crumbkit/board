# Kept decisions

This file holds detail for docs/architecture.md. It says how the index keeps
each mounted source's decision so a warm query or check neither walks nor
decides it. The working tree's decision is kept as [warm.md](warm.md) says.

## What is kept

- `decided`: one row per entry of a source, as a query's views show it,
  under the source's key. The source is not kept: each run attaches the
  source that reached the key. A mounted source's rows keep no `change`,
  as every entry of a commit is committed. Its
  `detail` column holds, as JSON, what `check` reports beyond the row: a
  degraded entry's detail and the schema it names. The `decided` view
  leaves it out.
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

A run opens a mounted repository and looks up its pinned commit only when
the repository's stamp changed ([warm.md](warm.md)), so a deleted repository
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
- `refs` decides every source, and keeps what it decided.
- A scoped run decides the working tree only in part, and keeps none of it.
- A scoped `ls` walks and decides only the sources its scope reaches, and
  keeps nothing for a source it decided only in part.
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

- one with a file that could not be read, or a read or resolution that
  failed with a git error, which may pass;
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
