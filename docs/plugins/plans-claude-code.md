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

When the session starts, the mod runs
`board --json call plans.open --limit 100`. The band above the prompt then
says how many questions are open, such as "3 open questions for you", with a
Show button. With none open, or while a survey holds the band, it draws
nothing. From the first time the band is drawn, the mod runs the call again
every minute, so a `-p` run, which draws no band, reads Board once. The
refresh does not depend on `isInteractive`, because desktop sessions run
through the Agent SDK and report it as false. A failed refresh is logged and
keeps the last count read. The band is drawn on the terminal and the desktop app only.

Show opens the `questions` pane. It lists each question with its project, its
age and what it blocks, and each option as a button; the recommended option is
drawn as the primary one. Where the surface draws a text field, which mobile
does not, the owner may answer in words instead. Pressing an option records
that option's `id` as `choice` and its text as `words`. Words typed in the
field record `choice: "none"` and the words verbatim.

Before it writes, the mod reads the question and every `plans/answer/v1`
whose `question` names it, at any id. If an answer exists, the mod writes
nothing and the toast names that answer's path. If the question's `status` is
no longer `open`, for example because it was withdrawn, the mod writes
nothing and the toast says the status.

An answer to an open question is one `board put --batch --wait 2s` of two
lines: a new `plans/answer/v1` at the question's id under `answers/`, which
must not exist yet, and the question with `status: "answered"`, which must
still hold the blob the mod read. Board writes both or neither, so a question
that changed after the read is refused whole. The mod then reads the question
again, and the toast names an answer written meanwhile, or else gives Board's
refusal. The put waits at most 2 seconds for board's lock, so a held lock is
refused as a Board failure before the mod's 5-second process timeout.

The answer carries `written_by: "plans/pane-answer"`. In the desktop app it
also carries `by`, this session's `local_` id from
`CLAUDE_CODE_HOST_SESSION_ID`, the same form questions use. Elsewhere no
`local_` id names the session, so `by` is left out. Answering needs no Touch
ID, because the press is the owner's own.

The answer is recorded in Board whether or not the asking session hears of
it. After the write, the mod tries to send the asking session a message
naming the question, the choice, the words and the answer's path, with
`$.session.send({ to: session })`, the question's `local_` id as a string.
The send is best effort. `{ sessionId }` is not used, because a headless probe
on 2.1.308 showed it refuses a live session's `local_` id. The string is the
SendMessage tool's spelling of a recipient; it was refused in the same
headless probe, and its delivery in the desktop app has not been tested. When
the send is refused, the toast says the answer is recorded in Board and the
message was not delivered.

## The morning report

`/yesterday` runs one `board query` for the day before the current one, from
07:00 local time to 07:00 today, and opens the `yesterday` pane. SQLite works
out the window from board's clock, daylight saving included. The pane lists,
as Markdown, the decisions made (`decisions/decision/v1`), the questions
asked, the questions answered and the runs recorded in that window. The command's output row gives the
totals. The query reads at most 1000 records, and when Board cuts it there,
both the pane and the output row say so. The report quotes record fields only and calls no model.

The pane is drawn by the mod's own `ui.render` hook. Once board-ui lands
(#301), it moves to board-ui view and binding records.
