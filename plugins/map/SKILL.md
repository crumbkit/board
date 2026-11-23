---
name: map
description: 'Maps of items, regions, typed lines and notes, each one map/layout/v1 record at /maps/<name>. Use it when you build or change a map, before you publish it. Run `board map check --path /maps/<name>`, which lists what a reader would see as broken, such as overlapping labels and lines through labels, with the box it estimated for each text, `board map place --path /maps/<name>`, which chooses positions and label sides for items that have none, and `board map page --path /maps/<name>`, which returns the map as one HTML page. SKILL.md and README.md in the plugin''s directory say more.'
---

# map

A map is one `map/layout/v1` record at `/maps/<name>`. It holds the sheet,
titled regions, items with a stored position and label side, typed lines
between items, and notes anchored to an item or a line.

## Check a map

Run `board map check --path /maps/<name>` after every change to a position,
a label side, a label or a note. It gives `problems`, the count, and
`findings`, each with a `kind`, a `message` and `estimated`, the box it
estimated for each label, note or region title the finding names.

Fix each finding by moving an item, choosing another label side, moving a
note along its line or across it, or shortening a label. Then check again.
Publish a map only when `problems` is 0.

## Place a map

Give each item its `region` and `label`, leave out `at`, and run
`board map place --path /maps/<name>`. It chooses each missing position and
label side together, and each line note's place along its line, so that
`check` finds nothing, then writes the map. Items it places are `engine`;
set `at.set` to `pinned` on an item you want kept where it is. Run it again
after adding items: it places only what has no position. Use `--relayout`
to choose engine positions again. When it is refused with
`map::place::findings_left`, it has stored its best spots; pin the item a
finding names where it belongs, or accept the finding, and run it again.

## Draw a map

Run `board map page --path /maps/<name>`. Its `html` is one page with no
network requests, which you save where the page belongs. Add a `ramp` and an
item `bin` to shade marks, `line_kinds` to style each kind of line, and
`tables` to show figures under the map. Items with no `at` are left out and
listed in `unplaced`.

## Lanes and columns

Give an item a `lane`, the id of a region, to keep it in that region's band,
and a `column`, a number from 0, to order a lane's items from left to right.
`place` then puts a lower column left of a higher one in the same lane. Items
that share a column are not ordered. A `lane` and a `region` on one item must
name the same region.

## Accept a finding

When a finding is part of the design, such as two lines that must leave an
item in one direction, add it to the map's `accepts` with its `kind`, its
`message` word for word, and a `reason`. `check` counts it in `accepted`
and stops listing it. When the map changes so the finding is gone, `check`
lists the accept as `accept-unused`; remove it.

## Text widths

`check` estimates text width from the number of characters, because Board
has no font. A label that passes may still touch a line on the page. When
the page and a finding disagree, compare the drawn text with the finding's
`estimated` box.

## Codes

- `map::place::findings_left`: placement stored its best spots and findings
  remain; the `result` field lists them.
- `map::layout::not_found`: no record is at the path. `board ls /maps`
  lists the maps.
- `map::layout::unreadable`: the record is not a `map/layout/v1`, or it
  breaks the schema, such as an item with an empty label. The message
  names the field.
- `map::layout::unknown_id`: an item, lane, line or note names a region, item or
  line the map does not hold, or a `line_kinds` entry names a kind no line
  has.
- `map::layout::duplicate_id`: two items, regions, line kinds or tables
  share a name.
- `map::layout::lane_region_mismatch`: an item's lane and region differ.
- `map::layout::bin_out_of_range`: an item's `bin` is outside the ramp.
- `map::layout::table_shape`: a table row's cells do not match its columns.
- `map::board::refused`: Board refused to read the path; the refusal
  names Board's own code in `code`. Run `board get` on the path to see it.
