# The plans mod for Claude Code

This file holds detail for docs/plugins.md. It says what the Claude Code mod
in `plugins/plans/claude-code/` shows the owner and what it writes.
`plugins/plans/README.md` says how to load it.

Agent sessions file the questions they need the owner to answer as
`plans/question/v1` records. The mod puts those questions in front of the
owner in any session, so no session has to relay them.

## Where it reads

The mod runs `board --personal`, because the owner's questions live in the
personal board. Its `board` option names another board's directory instead.
That board needs the plans plugin enabled, since the mod reads questions
through `plans.open`. When the call fails, the band draws nothing and the
debug log gives the code, such as `board::plugin::not_installed`.

## Open questions

When the session starts, and every minute after, the mod runs
`board --json call plans.open --limit 100`. The band above the prompt then
says how many questions are open, such as "3 open questions for you", with a
Show button. With none open, or while a survey holds the band, it draws
nothing. The band is drawn on the terminal and the desktop app only.

Show opens the `questions` pane. It lists each question with its project, its
age and what it blocks, and each option as a button; the recommended option is
drawn as the primary one. Where the surface draws a text field, which mobile
does not, the owner may answer in words instead. Pressing an option records
that option's `id` as `choice` and its text as `words`. Words typed in the
field record `choice: "none"` and the words verbatim.

An answer is one `board put --batch` of two lines: a new
`plans/answer/v1` at the question's id under `answers/`, written with
`--expect none`, and the question with `status: "answered"`, expected to hold
the blob the mod read just before. Board writes both or neither, so a
question answered elsewhere first is refused whole, and the toast gives the
refusal. The answer carries `by`, this session's id, and
`written_by: "plans/pane-answer"`. Answering needs no Touch ID, because the
press is the owner's own.

After the write, the mod sends the asking session a message naming the
question, the choice, the words and the answer's path, with
`$.session.send({ to: { sessionId } })`. A session that is not running, or
that this session cannot address, does not get the message; the toast says
so, and that session finds the answer when it next reads Board.

## The morning report

`/yesterday` runs one `board query` for the day before the current one, from
07:00 local time to 07:00 today, and opens the `yesterday` pane. SQLite works
out the window from board's clock, daylight saving included. The pane lists,
as Markdown, the decisions made, the questions asked, the questions answered
and the runs recorded in that window, and every decision stub that still has
an empty `why`, whenever it was written. The command's output row gives the
totals. The report quotes record fields only and calls no model.

The pane is drawn by the mod's own `ui.render` hook. Once board-ui lands
(viitta#301), it moves to board-ui view and binding records.
