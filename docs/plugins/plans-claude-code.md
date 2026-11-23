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
through `plans.open`. When the call fails, the debug log gives the code.

## Open questions

When the session starts, the mod runs `plans.open --limit 100` and
`plans.waiting`; [claude-code/band.md](claude-code/band.md) describes its
button. After that:

- Each tick runs `plans.waiting` if the band was drawn since the last read.
- A draw runs it when that read is a minute old.
- `plans.open` runs for a pane whose list is a minute old, a changed count, an answer, or every read without `plans.waiting`.

An undrawn session reads Board once. A failed refresh is logged, keeps
the last count read and marks the band's button. The pane says why for a
failed `plans.open`; a failed `plans.waiting` is in the debug log. The band is drawn on the terminal and the desktop app only.

The button opens the `questions` pane. It lists each question with its project, its
age and what it blocks, shown as the last path segment of `blocks` or as
`blocked_work`, and each option as a button; the recommended option is
drawn as the primary one. Where the surface draws a text field, which mobile
does not, the owner may answer in words instead. Pressing an option records
that option's `id` as `choice` and its text as `words`. Words typed in the
field record the words verbatim and no `choice`, unless the question has an
option whose id is `none`; then they record `choice` `none`.

The mod records the answer with `board --json call plans.answer`, passing
each input as a `--name=value` flag, so words that start with `-` stay the
value. When `plans.answer` refuses, it writes nothing and the toast gives its
code. When it attaches the answer to the change the question blocks and that
attach is refused, the toast names the change and the code.

The call may run for 15 seconds, since `plans.answer` takes board's lock
twice and each take waits up to 5 seconds; other `board` runs stop after 5.
When the call times out or ends without Board's failure report, the answer
may have been written. The toast says so, nothing is sent, and the question
stays listed until `plans.open` stops listing it.

The answer carries `written_by: "plans/pane-answer"`. In the desktop app the
mod also passes `--session` with this session's id from
`CLAUDE_CODE_HOST_SESSION_ID`, so the answer's `by` is
`{"session": <local_ id>}`, the same form questions use. Elsewhere no
`local_` id names the session, so `by` is left out. Answering needs no Touch
ID, because the press is the owner's own.

The answer is recorded in Board whether or not the asking session hears of
it. After the write, the mod sends the asking session, and each session in
`also_asked`, the question, the choice, the words and the answer's path, with
`$.session.send({ to })`, each `local_` id as a string.
The send is best effort. `{ sessionId }` is not used, because a headless probe
on 2.1.308 showed it refuses a live session's `local_` id. The string is the
SendMessage tool's spelling of a recipient; it was refused in the same
headless probe, and its delivery in the desktop app has not been tested. When
the send to the asking session is refused, the toast says the answer is
recorded in Board and the message was not delivered. When a person asked
without a session, the toast says so and names any `also_asked` session
sent the answer.

## The morning report

`/yesterday` runs one `board query` for the day before the current one, from
07:00 local time to 07:00 today, and opens the `yesterday` pane. SQLite works
out the window from board's clock, daylight saving included. The pane lists,
as Markdown, the decisions made, the questions asked, the questions answered
and the runs recorded in that window, and every decision stub that still has
an empty `why`, whenever it was written. Each question asked shows
`answered` when an answer names it, and its status otherwise. The command's output row gives the
totals. The query reads at most 1000 records, and when Board cuts it there,
both the pane and the output row say so. The report quotes record fields only and calls no model.

The pane is drawn by the mod's own `ui.render` hook. Once board-ui lands
(#301), it moves to board-ui view and binding records.
