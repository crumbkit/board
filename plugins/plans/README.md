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
  `question`, `session` (the asking session's `local_` id), `asked` and
  `status` (`open` or `withdrawn`) are required. `options`,
  `recommendation`, `why`, `blocks`, `mount`, `context_path`, `note` and
  `withdrawn_reason` are optional. A question is answered when a
  `plans/answer/v1` names it in `question`; answering leaves its status
  `open`. A reference is read from the root of the board it is in, so an
  answer in a mounted board names its question by the question's path in
  that board.
- `plans/answer/v1` at `/projects/<project>/answers/<id>`, with its question's
  id. `question` references the question; `choice` names an option's `id`, or
  `none`; `words` holds the owner's words verbatim; `answered` is when.
  `written_by` names the plugin and hook, or the client and surface, that
  wrote the answer, such as `plans/pane-answer` or `board-app/phone`.
- `plans/decision/v1` at `/projects/<project>/decisions/<id>`. `decision`,
  `why`, `session` and `decided` are required; `alternatives`, `refs`,
  `urls`, `review` (`wanted` or `none`) and `written_by` are optional. `refs` lists board
  entries the decision concerns, such as an answer; `urls` lists web pages,
  such as a pull request.

```json
{
  "$type": "plans/decision/v1",
  "decision": "Ship the plans types unchanged.",
  "why": "Records already in the personal board must stay valid.",
  "session": "local_335a90f9",
  "decided": "2026-10-25T12:00:00Z",
  "review": "wanted"
}
```

`why` must not be empty, except in a stub that a hook wrote without an agent.
Such a stub names its plugin and hook in `written_by`, such as
`runs/pr-create`, and the agent fills in `why` on its next turn.

## Capabilities

`board call plans.open [--path /projects/<project>] [--limit <n>]` lists the
questions below the path whose `status` is `open` and that no answer names, oldest first, with their options,
recommendation and what each blocks. It returns at most 100 by default, with
`total`, which counts the readable ones, and `truncated`. `unreadable` counts
open questions Board marks degraded or this plugin cannot read. Each question
also carries `why`, `note`, `mount` and `context_path`, null when the record
leaves them out.

`board call plans.answer --path <question> --option <id> [--words <text>]
[--by <session>] [--written_by <plugin>/<client>]` records the owner's answer
as the Claude Code mod's pane does, so a client without the mod, such as
Board.app, can answer. `--option` is one of the question's option ids, or
`none` with `--words`; a question with an option whose id is `none` takes
`--option none` as that option. `--words` defaults to the chosen option's
text. It writes one commit of two lines: a new `plans/answer/v1` at the
question's id under `answers/`, answered now, whose `question` names the
question by its path in the board the answer lands in, and the question with its content unchanged,
which must still hold the blob the call read. It returns the answer's path,
the question's path, `choice`, `words` and `answered`.

It writes nothing, and fails with its own code, when the path is not
`.../questions/<id>` (`plans::answer::not_a_question_path`), nothing there is
a question (`plans::answer::no_question`), an answer already names the
question (`plans::answer::already_answered`), the question's status is not
`open` (`plans::answer::not_open`), the option is not one of the question's
(`plans::answer::unknown_option`), or the option is `none` without words
(`plans::answer::words_needed`). A question changed after the read fails with
`plans::board::refused`, carrying Board's `board::put::stale`. It asks for no
Touch ID, as the pane does not.

`board call plans.withdraw --path <question> --reason <text>` withdraws a
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
