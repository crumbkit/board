---
name: priorities
description: 'The owner''s priorities, ranked in a short and a long horizon, in one record at /priorities. Use it to say what the owner''s priorities are and to keep them current.'
---

# priorities

A board holds at most one priorities record, at `/priorities`. Its `short`
list holds this week's priorities and its `long` list those beyond it,
highest first. Each holds at most 7 entries.

## Say the priorities

Run `board priorities list`. Give the entries in rank order. Name each
entry that shows `unconfirmed: true` or `stale: true`, and ask the owner to
confirm or drop it.

## Keep them current

- Add or edit an entry with `board priorities set --horizon short --id
  <id> --title '<one line>'`. Add `--at <rank>` to place it. To name the
  tasks and changes it covers, pass the whole input as JSON on stdin with
  `covers`, because flags cannot repeat:
  `echo '{"horizon": "short", "id": "<id>", "title": "<title>", "covers":
  ["/tasks/x.md"]}' | board call priorities.set`.
- Reorder with `board priorities move --id <id> --to <rank>`, and remove
  with `board priorities drop --id <id>`.
- A new entry, and one whose title or covers changed, shows as unconfirmed.
  When the owner confirms one, run `board priorities confirm --id <id>`, or
  `--all` when they confirm the whole list.

An entry covers only records in its own board. When the work lives in
another board, cover a local task that stands for it, or nothing.

## Codes

- `priorities::horizon::full`: the horizon holds 7 entries. Merge related
  entries, move one to the other horizon, or ask the owner which to drop.
  Only the owner's recorded decision raises the cap.
- `priorities::covers::too_many`: cover the project or parent task instead.
- `priorities::covers::through_mount` and `priorities::covers::wrong_type`:
  cover a task or change in this board.
- `priorities::id::invalid` and `priorities::id::not_found`: run `board
  priorities list` for the ids.
