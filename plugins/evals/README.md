# evals

A Board plugin for recording tests of libraries, tools, models and agents. An
eval states a question, the suites that test it and the runs its verdict
rests on. It is stored in a board, so an agent can ask what has already been
tested before it tests again. `SKILL.md` says which board an eval belongs in.

The suites and runs are the `runs` plugin's records, so a board that holds
evals enables both plugins. The runs README describes them.

## The eval record

An eval is a file `/evals/<name>.json`. Its evidence sits beside it, in
`/evals/<name>/evidence/`.

```json
{
  "$type": "evals/eval/v1",
  "question": "Can gix resolve a ref faster than git2?",
  "subjects": ["gix", "git2"],
  "repos": ["/repos/viitta"],
  "suites": ["/suites/gix-refs"],
  "runs": ["/suites/gix-refs/runs/20261001T090000Z-a3f9"],
  "decision": "Which git library board-tree uses.",
  "status": "concluded",
  "verdict": "supported",
  "conclusion": "gix resolved 1,000 refs in 41 ms against 63 ms for git2."
}
```

- `question`, `subjects` and `status` are required.
- `subjects` are the names of what is tested, without versions, such as `gix`
  or a model family. A repository the eval concerns goes in `repos`, by its
  mount, and never as `owner/name` here. `evals.about` matches subjects
  without regard to case. A run holds the versions used.
- `repos` references the mount of each repository the eval concerns, such as
  `/repos/viitta`, a `board/mount/v1` record at `/repos/viitta.json`. `check`
  reports `board::ref::dangling` for a path the board does not have, and
  `board refs /repos/viitta` lists the evals that name it.
- `suites` references the `runs/suite/v1` records that hold the method, the
  cases and the measures.
- `runs` references the `runs/run/v1` records the verdict rests on. A run
  names only its suite, so `board refs` on a run lists the evals that cite it.
- `check` reports `board::ref::dangling` or `board::ref::wrong_type` for a
  suite or run that reaches nothing or a record of another type.
- `decision` is the decision this eval informs.
- `status` is `planned`, `running`, `concluded` or `abandoned`. A `concluded`
  eval needs a `verdict` (`supported`, `refuted` or `inconclusive`); the
  schema refuses one without it. `conclusion` holds the prose.
- `written_by` names the plugin and hook that wrote the eval, such as
  `runs/test-run`, when a hook wrote it without an agent. An eval an agent or a
  person wrote leaves it out.

## Capabilities

`board call evals.about --subject gix` returns the evals that name that
subject, whatever their status. `board call evals.about --repo /repos/viitta`
returns the evals whose `repos` include that exact path, and the
evals stored in that mounted board, whose paths start with `/repos/viitta/`.
Board refuses input with both or neither as `board::plugin::bad_input`.
`board call evals.open` returns the evals whose status is
`planned` or `running`. A `concluded` eval without a verdict is not valid, so
neither lists it.

Both take an optional `limit`, 1 to 100 (default 100), and return
`{"evals": [...], "total": n, "truncated": bool}`. Each eval is its `path`,
`question`, `subjects`, `status`, `verdict` and `decision`, ordered by path.
Board checks the limit before the plugin runs. Case folding is SQLite's
`lower`, which folds ASCII letters only.

Each capability makes one `board query` over the `records` view, using
`json_each` and `json_extract` on the record body. The plugin extracts nothing
and writes nothing to its database. It lists an eval whose only fault is a
dangling or wrong-typed reference, and leaves out one that Board marks
degraded for any other reason. It also leaves out an eval that a board's own
looser schema admits but the plugin's model refuses.

## Across repositories

The query reads the `records` view, which includes every mounted board, so
the capabilities answer across repositories without knowing about mounts. A
personal board mounts each repository at `/repos/<name>`, and `about` then
lists `/repos/<name>/evals/<eval>`. `board ls /repos` lists the mounts. A
mounted board's own schemas govern its records, so a repository that holds
evals must enable `runs` and `evals` and commit the result; an eval it
cannot validate is not listed. A mount is pinned to a commit, so run
`board mount update /repos/<name>` to see new commits. The
personal board can put an eval at `/repos/<name>/evals/...` when the
repository's board sets `commit_on_put`, the mount's `follow` is a local
branch, and the repository's HEAD is on that branch and descends from the pin.
The put commits in the repository and moves the pin.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/evals"
    board plugin enable runs
    board plugin enable evals

`board plugin enable evals` writes the eval schema to `.board/types/evals/`
and names the plugin in `.board/board.json`. Install the `runs` plugin the
same way first.

The plugin's types stay at v1 and change in place while it has one user. After
an update that changes a schema, Board refuses `board call evals.*` with
`board::plugin::schema_differs` until the board holds the new schema. Run
`board plugin enable evals` again: it replaces the board's copy with the
plugin's, and `board check` lists the records the new schema refuses.

The directory also holds `SKILL.md`, the instructions an agent follows when it
tests something. Link the directory into the agent's skills to install it:

    ln -s "$PWD" ~/.claude/skills/evals

## Developing

The plugin talks to Board through the shared library in `python/board-plugin/`.
The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schemas are
generated from the pydantic models in `src/board_evals/models.py` by
`board_evals.manifest`, and a test checks that the committed files match.
