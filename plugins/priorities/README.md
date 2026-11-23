# priorities

A Board plugin that keeps the owner's priorities as one typed record,
ranked in two horizons. A lead agent reads it to say what the priorities
are, and keeps it current between conversations.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable priorities

This writes the schema it ships to `.board/types/priorities/`.

## Records

`priorities/list/v1` sits at `/priorities`:

```json
{
  "$type": "priorities/list/v1",
  "short": [
    {
      "id": "board-release",
      "title": "Board 0.1.0 release with the signed dmg",
      "covers": ["/tasks/board-release.md"],
      "confirmed": "2026-11-23"
    }
  ],
  "long": []
}
```

- `short` holds this week's priorities and `long` those beyond it, highest
  first, at most 7 each. An entry's rank is its place in its list and is
  never stored.
- `id` names the entry. It stays the same when the entry's title changes
  or it moves, and is unique across both lists.
- `title` is one line of at most 25 words.
- `covers` names up to 20 `tasks/task/v2`, `tasks/task/v3` and
  `changes/change/v2` records. Its items carry `x-board-local`, so a path
  reached through a mount makes `board check` mark the record
  `board::ref::through_mount`. It may be empty.
- `confirmed` is the day the owner last confirmed the entry. It is absent
  until then.

The schema carries `"x-board-goes-with-share": false`, so sharing a task
an entry covers never copies the priorities record into another board.

## Capabilities

Every write is one put of `/priorities` that expects the blob the call
read, so a record changed in between is refused with `board::put::stale`
and the call can run again. A refused call writes nothing.

- `list [--horizon short|long]` gives `short` and `long`, each entry with
  `rank`, `horizon`, `id`, `title`, `confirmed`, `unconfirmed` and
  `stale`, and each covered record as `{path, type, title, status}`, where
  `status` is a task's status or a change's phase, or `{path, missing:
  true}`. With no record both lists are empty. `stale_after` is the number
  of commits an entry may see.
- `set --horizon <h> --id <id> --title <t> [--at <rank>]` adds an entry or
  replaces the one with that id, moving it between horizons when needed.
  `covers` goes on stdin with the rest of the input, because flags cannot
  repeat. Left out, a replaced entry keeps its covers. Left without `--at`,
  a replaced entry keeps its place and a new one goes last; a rank past the
  end puts it last. A change to the title or covers removes `confirmed`.
- `move --id <id> --to <rank>` reorders an entry in its horizon, and `drop
  --id <id>` removes it. Both keep `confirmed` on the entries they leave.
- `confirm --id <id>` or `confirm --all` sets `confirmed` to today's UTC
  day.

`set`, `move` and `drop` give the entry's `id`, `horizon`, `rank` and
`unconfirmed`; `drop` gives the rank it had.

## Staleness

An entry is stale when the board's branch has more commits since the start
of its `confirmed` day, in UTC, than the params value
`priorities-reconfirm` allows, leaving out commits with a `Board-Bulk:`
trailer. The params plugin ships its heuristic with a default of 300. A
quiet board never marks an entry stale. When git cannot count the commits,
no entry is stale.

## Codes

- `priorities::id::invalid`: the id is not lowercase letters and digits in
  words joined by `-`.
- `priorities::id::not_found`: no entry has the id. The message lists the
  ids.
- `priorities::horizon::full`: the horizon holds 7 entries; the refusal
  names the lowest-ranked in `lowest`.
- `priorities::covers::too_many`: more than 20 covered records.
- `priorities::covers::wrong_type`: a covered path names no task or change.
- `priorities::covers::through_mount`: a covered path is in a mounted
  board.
- `priorities::confirm::unnamed`: `confirm` was given neither or both of
  `--id` and `--all`.
- `priorities::record::unreadable`: `/priorities` holds something the
  plugin cannot read.
