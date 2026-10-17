# evals

A Board plugin for recording tests of libraries, tools, models and agents. An
eval states a question and how it will be tested. Each run of it is a separate
record. Both are stored in a board, so an agent can ask what has already been
tested before it tests again. `SKILL.md` says which board an eval belongs in.

## The eval record

An eval is a file `/evals/<name>.json`. Its runs, evidence and cases file sit
in the directory beside it, `/evals/<name>/`.

```json
{
  "$type": "evals/eval/v1",
  "question": "Can gix resolve a ref faster than git2?",
  "subjects": ["gix", "git2"],
  "repos": ["/repos/viitta"],
  "method": "Resolve 1,000 refs in a clone of this repository and compare wall time.",
  "cases": [{"id": "cold", "input": {"refs": 1000}}],
  "measures": [{"name": "wall_ms", "unit": "ms"}],
  "decision": "Which git library board-tree uses.",
  "status": "planned"
}
```

- `question`, `subjects`, `method` and `status` are required.
- `subjects` are the names of what is tested, without versions, such as `gix`
  or a model family. A repository the eval concerns goes in `repos`, by its
  mount, and never as `owner/name` here. `evals.about` matches subjects
  without regard to case. Put the versions used in a run.
- `repos` references the mount of each repository the eval concerns, such as
  `/repos/viitta`, a `board/mount/v1` record at `/repos/viitta.json`. `check`
  reports `board::ref::dangling` for a path the board does not have, and
  `board refs /repos/viitta` lists the evals that name it.
- `cases` lists the cases inline; each has an `id`, which a run's results
  name, and optional `input` and `expected`. `cases_file` references a file
  that lists them instead, such as `/evals/<name>/cases.jsonl`.
- `measures` declares what each case may measure: a `name`, and optionally a
  `unit` and a `description`.
- `decision` is the decision this eval informs.
- `status` is `planned`, `running`, `concluded` or `abandoned`. A `concluded`
  eval needs a `verdict` (`supported`, `refuted` or `inconclusive`); the
  schema refuses one without it. `conclusion` holds the prose.

## The run record

A run is a file such as `/evals/<name>/runs/<id>.json`.

```json
{
  "$type": "evals/run/v1",
  "eval": "/evals/<name>",
  "by": "claude",
  "started": "2026-10-01T09:00:00Z",
  "versions": {"gix": "0.70.0", "git2": "0.20.0"},
  "results": [
    {"case": "cold", "trial": 1, "measures": {"wall_ms": 41.5}, "evidence": ["/evals/<name>/evidence/cold-1.txt"]}
  ],
  "measures": {"mean_wall_ms": 41.5}
}
```

- `eval`, `by`, `started` and `results` are required. `eval` references the
  eval, and Board reports `board::ref::dangling` or `board::ref::wrong_type`
  in `check` when it reaches nothing or something that is not an eval.
- `versions` maps what was used (a model, a prompt, a library) to its version.
- Each result names a `case` and has an `outcome` (`pass`, `fail`, `error` or
  `skipped`), `measures`, or both. A scored case that has no pass or fail
  gives `measures` alone, so the schema does not force a made-up outcome.
  `trial` numbers repeated trials of one case, from 1. `evidence` references
  raw files, such as logs.
- `measures` on the run holds aggregates. `finished`, `setup` and `summary`
  are optional prose and time.

The run has no counts of passes and failures. Count the results.

## Capabilities

`board call evals.about` takes `{"subject": "gix"}` and returns the evals that
name that subject, whatever their status. It takes `{"repo": "/repos/viitta"}`
instead and returns the evals whose `repos` include that exact path, and the
evals stored in that mounted board, whose paths start with `/repos/viitta/`.
Board refuses input with both or neither as `board::plugin::bad_input`.
`board call evals.open` takes `{}` and returns the evals whose status is
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

To list the runs of an eval, ask `board refs /evals/<name>`: it names every
record that references the eval.

## Across repositories

The query reads the `records` view, which includes every mounted board, so
the capabilities answer across repositories without knowing about mounts. A
personal board mounts each repository at `/repos/<name>`, and `about` then
lists `/repos/<name>/evals/<eval>`. `board ls /repos` lists the mounts. A
mounted board's own schemas govern its records, so a repository that holds
evals must run `board plugin enable evals` and commit the result; an eval it
cannot validate is not listed. A mount is pinned to a commit, so run
`board mount update /repos/<name>` to see new commits. The
personal board can put an eval at `/repos/<name>/evals/...` when the
repository's board sets `commit_on_put`, the mount's `follow` is a local
branch, and the repository's HEAD is on that branch and descends from the pin.
The put commits in the repository and moves the pin.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/evals"
    board plugin enable evals

`board plugin enable evals` writes the two schemas to `.board/types/evals/`
and names the plugin in `.board/board.json`.

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
