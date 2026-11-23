# Map

This file holds detail for docs/plugins/built-in.md. The `map` plugin keeps
a map as one record, `map/layout/v1`, at `/maps/<name>`, and its `check`
capability lists what a reader of the map would see as broken, its
`place` capability chooses positions and label sides, and its `page`
capability draws the map as an HTML page.
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

## What place does

`place` fills in what a map leaves out. It gives each item with no `at` a
position and a label side, and each line note with no `offset` a place along
its line. It tries every grid position (10 units apart) in the item's region
with each of the four sides, scores each with `check`, and keeps the spot with
the fewest findings, breaking ties by nearness to the item's line neighbours.
It then moves each placed item within 60 units to cut findings, and chooses a
side again for each item whose label lies within one label width of a placed
one. If findings remain it searches by simulated annealing, from a fixed seed
and in the order of the items' ids, so a map gives the same result whatever
order it lists its parts in.

A pinned item never moves, though its label side may change. An item the
engine placed before moves only with `relayout`. `place` writes the whole map
in one `put` that expects the blob it read, so a map changed meanwhile is
refused with `board::put::stale` and left as it was. When no spot clears every
finding, `place` stores the best spots and is refused with
`map::place::findings_left`.

## What page draws

`page` returns one HTML document in its output and writes no file, because
plugins leave file writes to the tree. The page has inline styles and a small
script and requests nothing from the network. It draws regions, each item as a
mark shaded by its `bin` in the map's `ramp`, labels on their stored sides,
lines in the style their `line_kinds` entry gives, notes from their anchors, a
key, and each table. Pointing at an item lights it, its lines and the table
rows that name it. The Board UI draws the same record with its `map` kind.
