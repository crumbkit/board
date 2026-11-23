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
  ],
  "ramp": {"label": "Open tasks", "bins": ["none", "1 to 3", "4+"]},
  "line_kinds": [{"kind": "reads", "label": "reads", "style": "dashed", "arrow": true}],
  "tables": [
    {"id": "tasks", "title": "Open tasks", "columns": [{"label": "Area"}, {"label": "Open", "numeric": true}],
     "rows": [{"item": "board", "cells": ["Board", "3"]}]}
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
- `lane` and `column` put an item in a band and an order. A `lane` names a
  region: the item stays in that region, as a `region` does, and the two
  must name the same region when both are given. A `column` (0 or more)
  orders the items of one lane from left to right: `place` keeps an item of
  a lower column left of an item of a higher column, and items sharing a
  column are not ordered. A `column` has no effect without a `lane`.
- `ramp` names a scale of 2 to 5 `bins`, lightest first. An item's `bin`
  (0 up to the last bin) shades its mark from the ramp, and the page draws
  the ramp as a key.
- `line_kinds` gives each line `kind` a `label`, a `style` (`solid`,
  `dashed` or `dotted`; `solid` when left out) and whether it has an `arrow`
  (true when left out). A kind with no entry is drawn solid with an arrow,
  and an entry for a kind no line has is refused.
- `tables` are tables drawn under the map. Each has an `id`, a `title`, an
  optional `caption`, `columns` (a `label` and whether the column is
  `numeric`) and `rows`, each with `cells`, one string for each column, and
  an optional `item` that the row lights when a reader points at it.

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
  chooses every line note again. After a person moves and pins an item, a
  run chooses the label sides within one label width of it again when a
  finding touches it. It gives `placed` (only the items it gave a
  position), `moved`,
  `sides_changed`, `notes_placed`, `problems` and `findings`. When
  findings are left it stores the best spots and is refused with
  `map::place::findings_left`, whose `result` field holds that output.

- `page --path /maps/<name>` gives `html`, one self-contained page that
  draws the map: regions, shaded items with their labels, lines in their
  kind's style, notes, the key and the tables. The page loads nothing from
  the network; its small script lights an item, its lines and its table
  rows when a reader points at one. It also gives `unplaced`, the ids of
  items with no `at`, which the page leaves out. The capability writes no
  file; save `html` where the page belongs.

## Codes

- `map::place::findings_left`: placement stored its best spots and
  findings remain. Pin the item a finding names, or accept the finding.
- `map::layout::not_found`: no record is at the path.
- `map::layout::unreadable`: the record is not a `map/layout/v1`, or it
  breaks the schema; the message names the field.
- `map::layout::unknown_id`: an item, lane, line, note or table row names
  what the map does not hold, or a `line_kinds` entry names a kind no line
  has.
- `map::layout::duplicate_id`: two items, regions, line kinds or tables
  share an id.
- `map::layout::lane_region_mismatch`: an item's `lane` and `region` name
  different regions.
- `map::layout::bin_out_of_range`: an item's `bin` is not below the
  ramp's bin count, or the map has no ramp.
- `map::layout::table_shape`: a table row has a different number of cells
  from the table's columns.
- `map::board::refused`: Board refused to read the path; the refusal
  names Board's own code in `code`. Run `board get` on the path to see it.
