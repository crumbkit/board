# The band above the prompt

This file holds detail for docs/plugins/claude-code.md. It says what the
band above the Claude Code prompt shows and how four mods draw it together.

The band is one row of badges, which wraps on a narrow terminal:

`Home work Board personal | [Orders 4] [Rules 5] [Ready to land 3] [Waiting 8 · 5 blocking] [Effects 2]`

A badge with nothing to show is left out. When every badge is left out, the
band draws nothing. While a survey holds the band, no mod draws in it.

## Badges

- **State badges**, from `board-rules`. Each is a dim label and its value
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
  `rules.check` does not report its board. With no such board, it is
  left out.
- **Orders button**, from `board-attention`: `Orders <n>`, the number of
  pinned orders that verified. It opens the pinned-orders pane, which lists
  every order; pressing one opens its text, path, expiry and signer, and a
  **Show context** button for its context. When orders are left out, the
  button shows in red as `Orders <n>: <m> left out`, and the pane gives each
  one's code and message. When the personal board cannot be read, the button
  shows in red with Board's code, such as `Orders:
  attention::signers::missing`, and the pane gives the message.
- **Rules button**, from `board-rules`: `Rules <n>`, the number of
  rules `rules.check` returned. It opens a pane that lists them as
  standing rules, obligations owed and budgets, each with its path and
  requirement. A rule with no checker is marked described. The pane ends by saying that editing a file adds a reminder
  for each rule on that file the session was not told about. A failed
  check shows the button in red with its code, such as `Rules:
  rules::rule::degraded`.
- **Ready to land button**, from `board-plans`: `Ready to land <n>`, in
  green, the changes `board --json call changes.ready` lists. Each is built
  and reviewed and waits only on a check from the owner, or is done and
  still owes him a recheck. With none, no button is drawn. It reads in the
  same refresh as Waiting, for up to 5 seconds. On a board with no changes
  plugin, or one whose plugin has no `ready` capability, there is no button
  and no error, and the mod stops asking until it loads again. A failed read
  keeps the last count and turns the button red: `Ready to land <n>: the last
  read failed`, or `Ready to land could not be read` when none has worked.
  The button opens the Ready to land pane. The pane lists the changes oldest
  first with the failing ones last. Each row gives the pull request, how long
  it has been ready, the kind of check (Recheck, Data sign-off, Deviation or
  Land choice), the title and what the check asks. An **Answer in /ask**
  button follows, and **Ask these now (/ask)** heads the pane. Both run /ask.
  The mod's `failing` option says whether a failing change is drawn in red:
  `silent`, the default, or `shown`. With `shown`, the button adds `· <m>
  failing` in red and leaves those changes out of `<n>`, and a failing row
  is tagged in red and names `board changes next --path <change>` in place
  of the /ask button. While the option is `silent`, failing changes count in
  `<n>`, and the mod appends a line to
  `$XDG_STATE_HOME/board/ready-failing.jsonl`, by default
  `~/.local/state/board/ready-failing.jsonl`. It does so each time a listed
  change starts failing, changes its reason or stops failing. Each line
  holds `at`, `change`, `pull`, `head`, `reason` and `session`. Lines older
  than 30 days are dropped on each write. A file that cannot be written
  records nothing, and the debug log says why.
- **Waiting button**, from `board-plans`: `Waiting <total> · <blocking>
  blocking` from `board --json call plans.waiting`, or `Waiting <total>`
  when nothing blocks. On a Board whose plans plugin has no `waiting`
  capability, Board refuses the call with `board::plugin::no_capability`,
  and the button counts the open questions instead, as `Questions <n>`. The mod then stops
  asking for `waiting` until it loads again. The button opens the open
  questions pane. When the read of the count the button draws fails, the
  button turns red and reads `<count>: the last read failed`, or `Questions
  could not be read` when none has worked. The pane says why only for a
  failed `plans.open`; a failed `plans.waiting` is in the debug log.
  `plans.waiting` leaves out the questions that block a change Ready to land
  counts, so a check is counted once. The pane lists those questions first,
  under a Ready to land heading.
- **Effects button**, from `board-effects`: `Effects <n>`, with `+` when
  Board listed only the first. It opens the pending effects pane. A failed
  read shows it in red, as `Effects could not be read`, or `Effects <n>: the
  last read failed` once a read has worked. Waiting counts only open
  questions, so effects are counted only here.

## How the mods share the row

The mods share nothing but the band. Claude Code asks each plugin that hooks
`AbovePrompt` for a row of its own, names that plugin in `e.plugin`, and
stacks the rows. Each mod's `ui.render` hook matches every plugin and draws
its part into the row of each of the four mods, so every Board row holds the
whole band. Each mod also hooks `AbovePromptList`, the stacking step. Of the
rows that hold a band, a Box keyed `board-band`, it keeps the one with the
most parts; every other row passes unchanged. This needs Claude Code 2.1.325 or later, the first to name the
plugin.

Within a row, each hook reads the parts of the hooks beneath it from the
tree `next(e)` returns. A part is a Box keyed `band:<slot>`. The
row is the Box keyed `board-band`, the first child of the band's column.
Each mod adds its parts to that row, sorts the parts by key, puts a dim `|`
between sections, and returns the whole band. The row is therefore the same
whichever mod runs first, and a mod that is not installed adds nothing. A
line that is not a badge, such as a request to sign an order, follows the
row. A tree beneath that is not a band, such as another plugin's, is kept as
a line after the row.

The slots, in order, are `a1-state`, `b1-orders`, `b2-controls`,
`b3-ready`, `b3-waiting` and `b4-effects`; the letter is the section. Each of the four
mods keeps an identical copy of `hooks/band.ts`, which holds this logic,
because a mod installs alone. The `mods` workflow fails when a copy differs
from the attention mod's, and the attention mod's `tests/band.test.ts` tests
the logic.
