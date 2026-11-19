# plans

A plugin built into Board for the questions an agent session asks the owner, the owner's
answers, and the decisions sessions record. Each schema's descriptions hold
the meaning of every field.

## Records

Each schema declares its folder, so `board put --new --var project=<project>`
names a new question or decision there. An answer takes its question's id,
so it is written at its path. Its schema declares its folder too, so
`put --new` can write an answer that has no question id.

- `plans/question/v1` at `/projects/<project>/questions/<id>`. `<id>` is a
  sortable UTC time and a short suffix, such as `20261024T1830Z-k3`.
  `question`, `by` (an `Actor` whose `session` is the asking session's
  `local_` id), `asked` and `status` (`open` or `withdrawn`) are required.
  `options`, `recommendation`, `why`, `blocks`, `blocked_work`, `mount`,
  `context_path`, `note`, `also_by` and `withdrawn_reason` are optional.
  `also_by` lists the other sessions, as `Actor`s, that `plans.ask` merged
  into the question. `blocks` is the
  path of the change or task that cannot proceed until the question is
  answered.
  `blocked_work` says what cannot proceed in words, when no change names it
  or the change needs more detail. A question is answered when a
  `plans/answer/v1` names it in `question`; answering leaves its status
  `open`. A reference is read from the root of the board it is in, so an
  answer in a mounted board names its question by the question's path in
  that board.
- `plans/answer/v1` at `/projects/<project>/answers/<id>`, with its question's
  id. `question` references the question; `choice` names an option's `id`, and
  is left out when the owner's words choose no option; `words` holds the owner's words verbatim; `answered` is when.
  `by`, optional, is the `Actor` who recorded it. `written_by` names the
  plugin and hook, or the client and surface, that wrote the answer, such as
  `plans/pane-answer` or `board-app/phone`.
- `plans/decision/v1` at `/projects/<project>/decisions/<id>`. `decision`,
  `why`, `by` (the deciding `Actor`) and `decided` are required; `alternatives`, `refs`,
  `urls`, `review` (`wanted`, or left out when no review is wanted) and `written_by` are optional. `refs` lists board
  entries the decision concerns, such as an answer; `urls` lists web pages,
  such as a pull request.

```json
{
  "$type": "plans/decision/v1",
  "decision": "Ship the plans types unchanged.",
  "why": "Records already in the personal board must stay valid.",
  "by": {"session": "local_335a90f9-0c1d-4e2f-8a9b-7c6d5e4f3a2b"},
  "decided": "2026-10-25T12:00:00Z",
  "review": "wanted"
}
```

`why` must not be empty, except in a stub that a hook wrote without an agent.
Such a stub names its plugin and hook in `written_by`, such as
`runs/pr-create`, and the agent fills in `why` on its next turn.

The changes plugin's owner checks count a decision as the owner's only when
its `written_by` is empty and its `by.person` is a string that is not empty.
They compare the person with no name, so any writer can set it. An agent that
records an approval the owner gave in chat names the owner as `person` and
quotes the owner's words in `why`:

```json
{
  "$type": "plans/decision/v1",
  "decision": "The owner approves the plan /changes/timeout/plans/1.",
  "why": "Asked whether he approves the plan, the owner answered \"Approve\".",
  "refs": ["/changes/timeout/plans/1"],
  "by": {"person": "pat", "session": "local_335a90f9-0c1d-4e2f-8a9b-7c6d5e4f3a2b"},
  "decided": "2026-11-15T22:30:00Z"
}
```

## Capabilities

`board plans open [--path /projects/<project>] [--limit <n>]` lists the
questions below the path whose `status` is `open` and that no answer names,
with their options, recommendation and what each blocks. Each question
carries `project`, the folder above its `questions/` folder, and `critical`,
true when `blocks` names a change whose phase is not `done` or `dropped`, or
a task whose status is `open`. A mounted question's `blocks` is read from its
own board's root. The questions come grouped by project. Within a project
the critical ones come first, then the rest, each oldest first. Projects
come in the order of their first question, so a project holding a critical
question comes before one that holds none. The limit keeps the first
ones. It returns at most 100 by default, with `total`, which counts the
readable ones, and `truncated`. `unreadable` counts
open questions Board marks degraded or this plugin cannot read. Each question
also carries `why`, `note`, `mount` and `context_path`, null when the record
leaves them out.

`board plans waiting [--path /projects/<project>]` counts the questions
`plans open` lists and returns `total`, `blocking` (how many of them are
critical), `unreadable` and `line`, one sentence an agent can end its reply
with, such as "3 questions are waiting for you, 2 of them blocking work. Run
/ask to answer them." The board-plans mod's Waiting button draws `total` and
`blocking`.

`board call plans.ask` with `{"project": "<project>", "question": {...}}` on
stdin files a question, the record as a `plans/question/v1` holds it, under
`/projects/<project>/questions/`, open and asked now unless it says when. It
first looks in the project for a question that asks the same words, matched
as `plans.sweep` matches them:

- An open one takes the asker into its `also_by` unless that session already
  asked it, and nothing is filed. `outcome` is `merged` and `path` names it.
- Otherwise, one that an answer names gives `outcome` `answered`, its `path`,
  the `answer` (`path`, `choice`, `words`) and `decisions`, the decisions
  whose `refs` name the question or the answer. Nothing is filed.
- Otherwise the question is filed: `outcome` is `filed` and `path` is new.

Each output carries `line`, one sentence that says what happened. A
question its schema refuses is refused with `plans::board::refused`,
carrying Board's code, and nothing is written. `plans.open` lists each
question's merged sessions in `also_asked`.

`board plans sweep [--path /projects/<project>] [--archived <local_ id>]...
[--apply true]` withdraws each open question below the path that no longer
needs the owner, with a reason that starts `plans.sweep:`. Each question is
withdrawn for the first of these that applies:

- `done` or `dropped`: the change or task `blocks` names is done or dropped.
- `archived`: the session that asked and every session in `also_by` are
  among `archived`. Board knows nothing of sessions, so the caller names
  the archived ones.
- `answered`: a question that an answer names, in the same project, asks
  the same words. Words match after lower-casing and keeping only letters
  and digits.

The sweep leaves open a question that repeats another open question,
because `plans.ask` merges repeats when they are filed.

Without `apply` it writes nothing. With it, each question is read again and
refused with `plans::sweep::<reason>`, for the reasons `plans.withdraw`
gives, if it was answered or withdrawn since the sweep read it, and nothing
is written. Otherwise every question of one board is withdrawn in one commit with
a `Board-Bulk: sweep questions` trailer. A batch may not cross a mount, so
each mounted board gets its own commit; when one is refused, the refusal
lists the boards already written in `written`. It returns `withdrawn` (at
most 100, each with `path`, `kind` and `reason`), `total`, `truncated`,
`counts` by kind and `remaining`, the open questions left.

`board plans answer --path <question> --option <id> [--words <text>]
[--session <local_ id>] [--program <slug>] [--person <slug>] [--agent <name>]
[--written_by <plugin>/<client>]` records the owner's answer. The Claude
Code mod's pane, Board.app and the ask skill all answer through it. `--option` is one of the question's option ids, or
`none` with `--words`; a question with an option whose id is `none` takes
`--option none` as that option. `--words` defaults to the chosen option's
text. It writes one commit of two lines: a new `plans/answer/v1` at the
question's id under `answers/`, answered now, with no `choice` when `none`
chooses no option, whose `question` names the
question by its path in the board the answer lands in, and the question with its content unchanged,
which must still hold the blob the call read. It returns the answer's path,
the question's path, `choice` (`none` when no option was chosen), `words`
and `answered`.

When the question's `blocks` names a change, read from the root of the
question's own board, the answer then calls `changes.attach --ref <answer>`
on that change, so the change's plan keeps the answer. The output's
`attached` names the change, and `attached.refused` gives the code when
`changes.attach` refused; the answer stays recorded either way. A question
that blocks a task or nothing gets no `attached`, since a task's Markdown
record has no list of references. A question in a mounted board gets no
`attached` either: `changes.attach` reads a change's plan from the root
board, so it cannot attach to a change in a mounted board.

It writes nothing, and fails with its own code, when the path is not
`.../questions/<id>` (`plans::answer::not_a_question_path`), nothing there is
a question (`plans::answer::no_question`), an answer already names the
question (`plans::answer::already_answered`), the question's status is not
`open` (`plans::answer::not_open`), the option is not one of the question's
(`plans::answer::unknown_option`), or the option is `none` without words
(`plans::answer::words_needed`). A question changed after the read fails with
`plans::board::refused`, carrying Board's `board::put::stale`. It asks for no
Touch ID, as the pane does not.

`board plans withdraw --path <question> --reason <text>` withdraws a
question that no longer matters: it rewrites the question with `status:
"withdrawn"` and the reason in `withdrawn_reason`, if it still holds the blob
the call read, and returns the question's path and the reason. It refuses
with `plans::withdraw::<reason>` for the same paths, missing questions,
answered questions and questions that are not open as `plans.answer` does,
and writes nothing then. An answer recorded between its read and its put
does not stop it, so such a question is both answered and withdrawn, and
`plans.open` does not list it.

## The Claude Code mod

`claude-code/` holds a Claude Code mod, `board-plans`. The band above the
prompt says how many questions are open, and its Show button opens a pane
where the owner answers each one. `/yesterday` opens a pane with the morning
report of the previous day from 07:00. Load it with
`claude --plugin-dir "$PWD/claude-code"`, or install it from the `board`
marketplace. It reads the personal board unless its `board` option names
another, and it needs this plugin enabled there.
[docs/plugins/plans-claude-code.md](../../docs/plugins/plans-claude-code.md)
explains the design.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable plans

An older install at `${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/plans`
makes `call` and `plugin enable` fail with `board::plugin::duplicate`.
Remove it; it is usually a symlink to a checkout, so remove the link, not the
checkout.

`board plugin enable plans` writes the three schemas to `.board/types/plans/`,
replacing any hand-kept copies, and names the plugin in `.board/board.json`.
The types stay at v1 and change in place while the plugin has one user; after
such a change, run `board plugin enable plans` again.

## Developing

The plugin is the crate `crates/board-plugin-plans`. It generates its record
schemas and the manifest's input and output schemas from Rust structs with
schemars. The record schemas are committed in its `types/` directory, and a
test fails when they differ from what the structs generate; run it with
`SNAPSHOTS=overwrite` to rewrite them.

This directory keeps the mod. The tests of the plugin through the binary are
in `crates/board-cli/src/tests/built_in/plans.rs`, which runs the same
commands in process.

`plans.migrate [--apply true]` rewrites a question whose `blocks` still holds
words. A value that names one change becomes that change's path, and its words
move to `blocked_work`. Any other value moves to `blocked_work`. A value that
is no text, or a question that already holds `blocked_work`, is listed under
`unread` with `plans::migrate::unread`. Without `--apply` it writes nothing.
With it, every rewrite lands in one commit whose trailer is `Board-Bulk:
migrate questions`. It reads only this board's own questions, so each nested
board runs it itself.
