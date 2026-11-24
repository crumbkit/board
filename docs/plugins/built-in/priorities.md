# Priorities

This file holds detail for docs/plugins/built-in.md. The `priorities`
plugin keeps the owner's priorities as one record, `priorities/list/v1`,
at `/priorities`. plugins/priorities/README.md lists its fields,
capabilities and codes.

## One record, two lists

The record holds two ordered lists, `short` for this week and `long` for
beyond it. An entry's rank is its place in its list, so a reorder is one
write that lands whole or not at all, and no stored rank can repeat or
skip. The horizon is the list itself, so no entry can carry a horizon that
disagrees with where it sits. Each list holds at most 7 entries, which the
schema and `set` both enforce. An entry's `id` stays the same through
edits and moves, so a later record can name one entry.

An entry's `covers` lists tasks and changes by typed reference. Its items
carry `x-board-local`, so every reference stays in the record's own board;
work kept in another board is covered by a local task, or not at all.

## Confirmation and staleness

An entry stores `confirmed`, the day the owner last confirmed it. `set`
removes it when the entry's title or covers change, so an agent may edit
the list and the owner still sees what they have not confirmed. `list`
marks an entry `stale` 7 days after that day, or sooner when the board's
commits since that day exceed the params value `priorities-reconfirm`.

## Sharing

The type's schema carries `"x-board-goes-with-share": false` at its top
level. The share plugin lists a back-ref of such a type as not copied, so
sharing a task an entry covers never copies the owner's ranking into
another board. Sharing `/priorities` itself still copies it.
