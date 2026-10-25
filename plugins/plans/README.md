# plans

A Board plugin for the questions an agent session asks the owner, the owner's
answers, and the decisions sessions record. Each schema's description holds
the full meaning of every field.

## Records

- `plans/question/v1` at `/projects/<project>/questions/<id>`. `<id>` is a
  sortable UTC time and a short suffix, such as `20261024T1830Z-k3`.
  `question`, `session` (the asking session's `local_` id), `asked` and
  `status` (`open`, `answered` or `withdrawn`) are required. `options`,
  `recommendation`, `why`, `blocks`, `repo`, `context_path` and `note` are
  optional.
- `plans/answer/v1` at `/projects/<project>/answers/<id>`, with its question's
  id. `question` references the question; `choice` names an option's `id`, or
  `none`; `words` holds the owner's words verbatim; `answered` is when.
- `plans/decision/v1`, wherever the writer keeps decisions, such as
  `/projects/<project>/decisions/<id>`. `decision`, `why`, `session` and
  `decided` are required; `alternatives`, `refs` and `review` (`wanted` or
  `none`) are optional.

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
questions below the path whose `status` is `open`, oldest first, with their options,
recommendation and what each blocks. It returns at most 100 by default, with
`total` and `truncated`. `unreadable` counts open questions Board marks
degraded or this plugin cannot read.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/plans"
    board plugin enable plans

`board plugin enable plans` writes the three schemas to `.board/types/plans/`,
replacing any hand-kept copies, and names the plugin in `.board/board.json`.
The types stay at v1 and change in place while the plugin has one user; after
such a change, run `board plugin enable plans` again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schemas are
generated from the models in `src/board_plans/models.py` by
`board_plans.manifest`, and a test checks that the committed files match.
