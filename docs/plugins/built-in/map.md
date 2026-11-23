# Map

This file holds detail for docs/plugins/built-in.md. The `map` plugin keeps
a map as one record, `map/layout/v1`, at `/maps/<name>`, and its `check`
capability lists what a reader of the map would see as broken.
plugins/map/README.md lists its fields, capabilities and codes.

## Stored positions and anchored notes

An item stores its centre and its label side, and says who set the
position: `engine` for placement, `pinned` for a person or agent. A note
stores no coordinates. It stores an item with an offset from the item's
centre, or a line with a fraction along it and an offset across it, and its
position is computed from its anchor whenever the map is read. A note
therefore stays with its item or line when that moves.

## What check reports

`check` computes each label's, note's and region title's box from stored
positions, and reports overlapping labels, labels over marks or region
titles, labels off the sheet or outside their region, lines through labels,
marks or region titles, two lines that leave one item within 0.14 radians of
each other, overlapping marks, notes more than 70 units from what they
describe, notes over labels, marks or other lines, and labels or notes
nearer another mark or line than their own. Where several marks or lines
are nearer, it names the nearest, with ties broken by name, so the result
does not depend on the order the map lists its parts in.

It estimates text width from the number of characters: 7.4 units for each
character of a label's first line, 6.5 for later lines, 6.6 for a note and
7.6 for a region title, with lines 15 units high. Board has no font, so a
finding gives the box it estimated for each text it names, which a reader
compares with the drawn page.

A map's `accepts` lists findings it keeps on purpose, matched by kind and
exact message. `check` counts them in `accepted` instead of listing them,
and lists an accept that matches no finding as `accept-unused`.

The constants and tests come from the map prototype's `geom.js`, which
three real maps were checked with. The plugin's fixtures keep those maps'
geometry and label lengths with every name replaced.
