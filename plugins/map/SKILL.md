---
name: map
description: 'Maps of items, regions, typed lines and notes, each one map/layout/v1 record at /maps/<name>. Use it when you build or change a map, before you publish it. Run `board map check --path /maps/<name>`, which lists what a reader would see as broken, such as overlapping labels and lines through labels, with the box it estimated for each text. SKILL.md and README.md in the plugin''s directory say more.'
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

- `map::layout::not_found`: no record is at the path. `board ls /maps`
  lists the maps.
- `map::layout::unreadable`: the record is not a `map/layout/v1`.
- `map::layout::unknown_id`: an item, line or note names a region, item or
  line the map does not hold.
- `map::layout::duplicate_id`: two items or two regions share a name.
