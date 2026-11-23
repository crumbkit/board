# map

A Board plugin that keeps a map as one typed record and checks it for what
a reader would see as broken.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable map

This writes the schema it ships to `.board/types/map/`.

## Records

`map/layout/v1` sits at `/maps/<name>`:

```json
{
  "$type": "map/layout/v1",
  "title": "Services and their links",
  "sheet": {"width": 720, "height": 520},
  "regions": [
    {"id": "core", "title": "Core", "x": 20, "y": 20, "width": 420, "height": 250}
  ],
  "items": [
    {"id": "board", "region": "core", "label": ["Board", "records in git"],
     "radius": 12, "at": {"x": 120, "y": 120, "set": "pinned"}, "side": "right"},
    {"id": "ui", "region": "core", "label": ["Board UI"],
     "at": {"x": 300, "y": 200, "set": "engine"}}
  ],
  "lines": [{"from": "ui", "to": "board", "kind": "reads"}],
  "notes": [
    {"text": "HTTP", "anchor": {"line": {"from": "ui", "to": "board", "along": 0.5}}}
  ],
  "accepts": [
    {"kind": "lines-share-angle", "message": "...", "reason": "..."}
  ]
}
```

Positions are in layout units, one to a CSS pixel at the sheet's own size,
with x to the right and y down from the sheet's top left corner.

- `sheet` is the area every label must stay on.
- `regions` are titled rectangles. Each item's label must stay inside its
  region, and no label or line may cross a region's title.
- `items` each have an `id`, an optional `region`, a `label` of one or more
  lines, an optional `radius` (8 when left out), an optional `at` and an
  optional `side` (`right` when left out; also `left`, `below`, `above`).
  `at.set` is `engine` when placement chose the position and `pinned` when
  a person or agent set it. An item with no `at` is not drawn.
- `lines` join two items and carry a `kind`, such as `library` or `after`.
- `notes` are short texts. A note anchored to an `item` stores `dx` and
  `dy` from the item's centre. A note anchored to a `line` stores `along`,
  from 0 at `from` to 1 at `to`, and `offset` across the line (9 when left
  out). A note's position is computed from its anchor, so it moves with
  what it describes.
- `accepts` lists findings the map keeps on purpose, each with a `reason`.

## Capabilities

- `check --path /maps/<name> [--limit <n>]` reads the map and gives
  `problems`, `findings` sorted by kind and message, `left_out` beyond the
  limit (100 when left out), and `accepted`. Each finding has `kind`,
  `message` and `estimated`, the box estimated for each label, note or
  region title it names. The kinds are `label-overlap`, `label-off-sheet`,
  `label-outside-region`, `label-nearer-other`, `line-through-label`,
  `line-through-item`, `lines-share-angle`, `items-overlap`,
  `item-unplaced`, `note-adrift`, `note-nearer-other`, `note-overlap` and
  `accept-unused`.

- `place --path /maps/<name> [--relayout] [--limit <n>]` chooses a
  position and label side for each item with no `at`, and a place along
  its line (`along` and `offset`) for each line note with no `offset`, by
  scoring every candidate with `check`. It writes the whole map in one
  `put`. A pinned item never moves, though its label side may change; an
  item the engine placed before moves only with `--relayout`, which also
  chooses every line note again. It gives `placed`, `moved`,
  `sides_changed`, `notes_placed`, `problems` and `findings`. When
  findings are left it stores the best spots and is refused with
  `map::place::findings_left`, whose `result` field holds that output.

## Codes

- `map::place::findings_left`: placement stored its best spots and
  findings remain. Pin the item a finding names, or accept the finding.
- `map::layout::not_found`: no record is at the path.
- `map::layout::unreadable`: the record is not a `map/layout/v1`, or it
  breaks the schema; the message names the field.
- `map::layout::unknown_id`: an item, line or note names what the map does
  not hold.
- `map::layout::duplicate_id`: two items or two regions share an id.
- `map::board::refused`: Board refused to read the path; the refusal
  names Board's own code in `code`. Run `board get` on the path to see it.
