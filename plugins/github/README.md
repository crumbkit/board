# github

A Board plugin that records GitHub pull requests and issues as typed records,
so other records can reference them. SKILL.md holds the procedure.

## Records

`github/pull/v1` at `/gh/<owner>/<repo>/pulls/<number>.json` and
`github/issue/v1` at `/gh/<owner>/<repo>/issues/<number>.json`.

```json
{
  "$type": "github/pull/v1",
  "owner": "octo",
  "repo": "widgets",
  "number": 12,
  "url": "https://github.com/octo/widgets/pull/12",
  "title": "Add a widget"
}
```

- `owner`, `repo`, `number` and `url` are required.
- `title` is the title when the record was written. The plugin keeps no other
  GitHub state and never updates the title.

## Capabilities

- `ref --ref <owner>/<repo>#<number>` writes the record. When a pull or issue
  record for that number is already there, it reports it and runs nothing
  else. Otherwise it runs `gh api repos/<owner>/<repo>/issues/<number>` once.
  GitHub answers that for pull requests too, and the answer's `pull_request`
  field says which it is. It then writes the record with an expect-none put.
  It returns `path`, `type`, `written`, `url` and `title`, which is null when
  the record holds none.
  - A reference that is not `owner/repo#number` is refused by Board with
    `board::plugin::bad_input`.
  - GitHub's 404 is `github::ref::not_found`.
  - gh missing, failing, running past 20 seconds or printing no `html_url`
    is `github::gh::failed`, with gh's error in the message.
  - A record already there that Board marks degraded is
    `github::record::degraded`.

The owner and repository are kept as given. GitHub matches them without case,
so `Octo/Widgets#12` and `octo/widgets#12` make two records.

`ref` passes gh `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` when Board has
them.

## Layout

Each capability is one module in `src/board_github/`, listed in
`__main__.py`. Every call to GitHub goes through `gh.py`. `objects.py` holds
the record types and their paths.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/github"
    board plugin enable github

Enabling writes the schemas to `.board/types/github/` and names the plugin in
`.board/board.json`.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. They put a stub gh first on PATH, so no test
reaches GitHub. `plugin.json` and the shipped schemas are generated from
`src/board_github/models.py` by `board_github.manifest`, and a test checks that
the committed files match.
