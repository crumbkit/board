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
  at the same id under `/effects/seen/` that Board reads as an answer: a
  string `effect`, an `answer` of `approve` or `refuse`, an optional string
  `reason`, and no other field. A seen record with two `answer` keys is the
  one case the query cannot check: SQLite and Board may read different
  ones. The query is in `hooks/query.ts`, and
  crates/board-cli/src/tests/effects_mod.rs runs it against a real board. It
  returns only the fields the mod shows, the first 2,000 characters of the
  record or text a put would write, and that content's size in bytes and
  lines, so no row carries a whole body. The mod reads the list when a
  session starts and after each answer. It also reads it every 60 seconds
  from the first time the band is drawn, which a `claude -p` run never does.
- When any are pending, the band above the prompt says how many, such as
  `2 effects wait for your answer`, with a Show button. When a read fails,
  the band says `Board: could not read pending effects`, followed by the
  count of the last read that worked, such as `(last read: 2)`, and the
  failure goes to the debug log. The band shows nothing when no read has
  worked and the failure is `board::config::not_found`, which means there is
  no personal board.
- The pane lists up to 100 effects. Each starts with a line such as
  `In board "~/work", notes asks to put "/notes/plan.md"`. The board's full
  root comes first, with the home directory written as `~`, and the path is
  quoted, because a plugin chooses the path and a path such as
  `/README.md in ~/Code/scratch` could otherwise name another board. Under it
  are the first lines of what a put would write. It says when the put replaces a file. When
  it shows only part of the content, it says how much is left out, such as
  `and 146 more lines, 12 KB in all`, and gives the command
  `board --personal get /effects/pending/<id>` that prints the effect whole.
  SQLite's text functions stop at a NUL character, so when the content holds
  one, the pane says so and that only the part before it is shown. For an op
  other than put, it says that an approval stands until the seen
  record is removed. Its Refuse and Approve buttons run
  `board answer <id> refuse` or `board answer <id> approve`. When a read
  fails, the pane says why in place of "No effect waits for your answer", and
  says when the list it shows is from the last read that worked.
- What the mod shows escapes the characters Board's own messages escape:
  control characters other than line feed and tab, Unicode format characters
  such as bidi overrides and zero-width spaces, line and paragraph
  separators, and backslash. A control character makes Claude Code refuse to
  draw the screen, and a format character could make one path read as
  another. The headline escapes line feed and tab too.
- Before it runs `board answer`, the mod reads that one effect again. If it
  is no longer pending, because it was answered elsewhere, a toast says no
  answer was sent. `board answer` overwrites an earlier answer, so without
  this check a stale row or dialog could replace a refusal with an approval.
  A second press while an answer runs runs nothing, and a toast says the
  answer is already running. The re-read can go once Board refuses a second
  answer.
- `board answer` has 30 seconds to finish. When Board reports a failure, a
  toast says that Board recorded no answer and gives Board's reason. When the
  run times out or is killed, a toast says that whether the answer was
  recorded is unknown. Either way the mod reads the list again.
- After a Bash call whose output holds `board::settings::asked`, the mod
  reads the list once, reads again each effect id the output names that the
  list holds, and asks the
  owner in a dialog with Later, Refuse and Approve, in that order, so a stray
  Enter answers nothing. It asks about each id once while the mod is loaded,
  and parallel calls that name one id open one dialog. When the read of the
  effect fails, the failure goes to the debug log and the id is asked about
  the next time output names it. The dialog shows the effect as
  Board recorded it, so output that names an effect Board does not hold
  pending opens no dialog. Approve or Refuse runs `board answer`, and the
  agent that ran the call then reads a line it alone sees: after an approval,
  that it may run the same put again; after a refusal, that the put is now
  refused. Later answers nothing, and the agent reads that the owner has not
  answered yet, so it should not retry the put or answer it itself. A dismissed dialog answers
  nothing.

The mod passes each id to `board` as its own argument, and an id is 64
lowercase hex digits before it reaches the query. Once `board answer` removes
the answered effect from `/effects/pending/`, the query no longer needs its
seen-record check.

The mod offers no way to save a rule. `board answer --save-rule` asks for no
Touch ID today, and saving a rule must be signed with Touch ID
(plan/board-ui.md, decision 12). When Board signs saved rules, the dialog and
the pane can offer "Approve and save rule".
