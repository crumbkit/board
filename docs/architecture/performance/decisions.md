# Kept decisions

This file holds detail for docs/architecture.md. It says how the index keeps
each mounted source's decision so a warm query neither walks nor decides it.

## What is kept

- `decided`: one row per entry of a mounted source, as a query's views show
  it, under the source's key. The source and `change` are not kept: a key
  names a tree, and each run attaches the commit that reached it.
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

Every run opens each mounted repository and looks up each pinned commit, so
a deleted repository or a lost commit changes the key. The repositories one
source's mounts name are opened in parallel, one thread each, before its
mounts are read in order.

## Who reads and who fills

- `query`, `call` and `enable` read kept rows in place and decide only the
  sources whose key the index does not hold.
- An unscoped `check`, and `refs`, decide every source, and keep what they
  decided.
- A scoped `check` and `ls` decide only the sources their scope reaches.
  They walk another source only when the index lists no pointers for it,
  to find its mounts. They keep nothing for a source they decided only in
  part. On a copy of the owner's board with 24 mounts, a warm `check
  /controls` takes about 38 ms instead of 59 ms because of this.

## What is never kept

These sources are decided on every run:

- one with a file that could not be read, or a read or resolution that
  failed with a git error, which may pass;
- one holding a mount whose route is too long to name;
- one that reaches a commit through another repository than the one that
  commit was walked in;
- one whose resolution crossed a mount that no probe holds, such as a
  pointer in a schema file;
- one holding a pointer whose blob cannot be read.

## Order and pruning

- `entries` lists the kept rows by source, then path, then the rest: the
  working tree's and those of sources never kept. Whether a source is kept
  depends on the source, not on the cache, so a query without `ORDER BY`
  answers alike warm and cold.
- A prune deletes kept rows and pointer lists the run did not reach.
