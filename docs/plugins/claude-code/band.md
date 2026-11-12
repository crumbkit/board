# The band above the prompt

This file holds detail for docs/plugins/claude-code.md. It says what the
band above the Claude Code prompt shows and how four mods draw it together.

The band is one row of badges, which wraps on a narrow terminal:

`Home work Board personal | [Orders 4] [Controls 5] [Questions 3] [Effects 2]`

A badge with nothing to show is left out. When every badge is left out, the
band draws nothing. While a survey holds the band, no mod draws in it.

## Badges

- **State badges**, from `board-controls`. Each is a dim label and its value
  drawn as a chip, and none can be pressed. The owner lists them in an optional file,
  `~/.config/board/badges.json` (or under `$XDG_CONFIG_HOME`), such as
  `{"badges": [{"label": "Home", "argv": ["home-name", "--short"]}]}`.
  Without the file there are none. A file that cannot be read or is
  malformed shows one red badge naming the file and the problem, such as
  `has no badges list` or `entry 2 needs a label and an argv of strings`. Each
  command runs once as the session starts, in the session's directory, with
  the same PATH as the mods' board calls and 2 seconds to finish. The first
  line of its standard output is the value. A command that fails, times out
  or prints nothing shows no badge, and the debug log says why. After
  them comes the built-in **Board** badge: `personal` when the session's
  directory is in the board that `personal` in `config.json` names, or a
  repository it mounts, else the
  name of the directory of the board that holds it. The mod reads that
  board from `board --json ls / --limit 1` at the repository root, because
  `controls.check` does not report its board. With no such board, it is
  left out.
- **Orders button**, from `board-attention`: `Orders <n>`, the number of
  pinned orders that verified. It opens the pinned-orders pane, which lists
  every order; pressing one opens its text, path, expiry and signer, and a
  **Show context** button for its context. When orders are left out, the
  button shows in red as `Orders <n>: <m> left out`, and the pane gives each
  one's code and message. When the personal board cannot be read, the button
  shows in red with Board's code, such as `Orders:
  attention::signers::missing`, and the pane gives the message.
- **Controls button**, from `board-controls`: `Controls <n>`, the number of
  controls `controls.check` returned. It opens a pane that lists them as
  standing rules, obligations owed and budgets, each with its path and
  requirement. The pane ends by saying that editing a file adds a reminder
  for each control on that file the session was not told about. A failed
  check shows the button in red with its code, such as `Controls:
  rules::rule::degraded`.
- **Waiting button**, from `board-plans`: `Waiting <total> · <blocking>
  blocking` from `board --json call plans.waiting`, or `Waiting <total>`
  when nothing blocks. While the plans plugin has no `waiting` capability,
  Board refuses the call with `board::plugin::no_capability`, and the button
  counts the open questions instead, as `Questions <n>`. The mod then stops
  asking for `waiting` until it loads again. The button opens the open
  questions pane.
- **Effects button**, from `board-effects`: `Effects <n>`, with `+` when
  Board listed only the first. It opens the pending effects pane. A failed
  read shows it in red, as `Effects could not be read`, or `Effects <n>: the
  last read failed` once a read has worked. The button stays after
  `plans.waiting` lands, and its effects are then also counted in Waiting.

## How the mods share the row

The mods share nothing but the band. Each draws its part in its own
`ui.render` hook on `AbovePrompt` and reads the parts of the mods beneath it
from the tree `next(e)` returns. A part is a Box keyed `band:<slot>`. The
row is the Box keyed `board-band`, the first child of the band's column.
Each mod adds its parts to that row, sorts the parts by key, puts a dim `|`
between sections, and returns the whole band. The row is therefore the same
whichever mod runs first, and a mod that is not installed adds nothing. A
line that is not a badge, such as a request to sign an order, follows the
row. A tree beneath that is not a band, such as another plugin's, is kept as
a line after the row.

The slots, in order, are `a1-state`, `b1-orders`, `b2-controls`,
`b3-waiting` and `b4-effects`; the letter is the section. Each of the four
mods keeps an identical copy of `hooks/band.ts`, which holds this logic,
because a mod installs alone. The `mods` workflow fails when a copy differs
from the attention mod's, and the attention mod's `tests/band.test.ts` tests
the logic.
