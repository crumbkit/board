# The effects mod

This file holds detail for docs/plugins/claude-code.md. It says what the
`board-effects` mod in `plugins/effects/claude-code/` shows the owner and
which commands it runs. docs/architecture/writes/effects.md describes the
effects themselves.

The mod lets the owner answer a plugin's pending effects without leaving
Claude Code. Board makes every decision and checks every answer. The mod only
lists effects and runs `board answer` for the owner, so it holds no rule of
its own.

- It lists the pending effects with one `board --personal query` over
  `/effects/pending/`, leaving out each effect with a `board/seen/v1` record
  at the same id under `/effects/seen/`. The query is in `hooks/query.ts`, and
  crates/board-cli/src/tests/effects_mod.rs runs it against a real board. It
  returns only the fields the mod shows, the first 2,000 characters of the
  record or text a put would write, and that content's size in bytes and
  lines, so no row carries a whole body. The mod reads the list when the
  session starts, every 60 seconds, and after each answer.
- When any are pending, the band above the prompt says how many, such as
  `2 effects wait for your answer`, with a Show button. When a read fails,
  the band keeps the last count it read, and the failure goes to the debug
  log.
- The pane lists up to 100 effects. Each shows the plugin, the op, the path,
  the board's full root with the home directory written as `~`, and the first
  lines of what a put would write. It says when the put replaces a file. When
  it shows only part of the content, it says how much is left out, such as
  `and 146 more lines, 12 KB in all`, and gives the command
  `board --personal get /effects/pending/<id>` that prints the effect whole.
  For an op other than put, it says that an approval stands until the seen
  record is removed. Its Refuse and Approve buttons run
  `board answer <id> refuse` or `board answer <id> approve`. When a read
  fails, the pane says why in place of "No effect waits for your answer", and
  says when the list it shows is from the last read that worked.
- Each control character other than line feed and tab in what the mod shows
  is written as `\xNN`, because Claude Code refuses to draw a screen that
  holds one.
- `board answer` has 30 seconds to finish. When Board reports a failure, a
  toast says that no answer was recorded, and the effect stays listed. When
  the run times out or is killed, a toast says that whether the answer was
  recorded is unknown, and the mod reads the list again.
- After a Bash call whose output holds `board::settings::asked`, the mod
  reads each effect id the output names from the personal board and asks the
  owner in a dialog with Later, Refuse and Approve, in that order, so a stray
  Enter answers nothing. It asks about each id once per session, so parallel
  calls that name one id open one dialog. The dialog shows the effect as
  Board recorded it, so output that names an effect Board does not hold
  pending opens no dialog. Approve or Refuse runs `board answer`, and the
  agent that ran the call then reads a line it alone sees: after an approval,
  that it may run the same put again; after a refusal, that the put is now
  refused. Later, or a dismissed dialog, answers nothing.

The mod passes each id to `board` as its own argument, and an id is 64
lowercase hex digits before it reaches the query. Once `board answer` removes
the answered effect from `/effects/pending/`, the query no longer needs its
seen-record check.

The mod offers no way to save a rule. `board answer --save-rule` asks for no
Touch ID today, and saving a rule must be signed with Touch ID
(plan/board-ui.md, decision 12). When Board signs saved rules, the dialog and
the pane can offer "Approve and save rule".
