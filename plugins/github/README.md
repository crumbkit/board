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
  - gh missing, failing, running past 20 seconds or answering without an
    `html_url` is `github::gh::failed`, with gh's error or the command that
    shows the answer in the message.
  - A record already there that Board marks degraded, such as one with
    capitals in `owner`, is `github::record::degraded`.

Owner and repository are stored in lower case, in the path and the fields,
because GitHub matches them without case; `url` keeps GitHub's own case. The
record keeps the path it was asked under, and `url` is where GitHub answered
from, which differs after a repository is renamed or an issue transferred.

`ref` declares `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` as its `env`,
so gh sees them when Board has them, with the variables any plugin's process
sees.

## Layout

The plugin is built into Board as the crate `crates/board-plugin-github`.
Each capability is one module in its `src/`, listed in `CAPABILITIES` in
`lib.rs`. Every call to GitHub goes through `gh.rs`, which runs gh through
the host with a 20-second deadline. `objects.rs` holds the record kinds and
their paths, and `records.rs` the record types.

## Installing

It is built into Board. `board plugin enable github` writes the schemas to
`.board/types/github/` and names the plugin in `.board/board.json`. An older
install at `${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/github` makes
`call` and `plugin enable` fail with `board::plugin::duplicate`. Remove it;
it is usually a symlink to a checkout, so remove the link, not the checkout.

## Developing

The shipped schemas are generated from `src/records.rs`, and the files under
the crate's `types/` keep the bytes the Python plugin shipped. A test checks
that they match; `SNAPSHOTS=overwrite` rewrites them. The tests of the plugin
through the binary are in `crates/board-cli/src/tests/built_in/github.rs`.
They put a stub gh first on the PATH the plugin's process sees, so no test
reaches GitHub.
