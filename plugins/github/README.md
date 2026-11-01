# github

A Board plugin that records GitHub pull requests and issues as typed records,
so other records can reference them, and sends content to GitHub only after
the boundary plugin passes it. SKILL.md holds the procedure.

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

### Sending actions

`push`, `pr-create`, `pr-edit`, `comment` and `issue-create` send content
from the checkout Board was started in, which Board names in
`BOARD_CALLER_DIR`. Each action works out where the content goes, collects
it as named parts, and passes them to
`board --personal call boundary.check` as the action `github.<name>`. It
sends only when the check passes. Then it records what GitHub returned with
`boundary.record`, and returns `digest`, `crossing`, `note` and `sent`, the
result it recorded.

- `push` resolves the branch checked out to its commit once, before the
  check, and pushes that commit to the branch of the same name on origin, so
  a commit made during the check is not sent. It pushes no tags, no submodule
  commits and no push options, whatever git's config says, then sets the
  branch's upstream. It returns `branch`, `commits`, the commits sent, oldest
  first, and `sent`, git's line for the branch, such as
  `<sha>:refs/heads/x [new branch]`.
- `pr-create --title <t> --body <b> [--base <branch>] [--draft true]` pushes
  as `push` does, then runs `gh pr create`. One check covers the commits,
  title and body. It records git's line on the crossing as soon as the push
  lands, so a gh failure after it still leaves the push recorded. It writes
  the `github/pull/v1` record and returns `number` and `path`; `sent` is the
  pull request's URL, which replaces git's line on the crossing.
- `pr-edit --number <n> [--title <t>] [--body <b>]` changes the title, the
  body or both through `gh api`. Board refuses a call with neither as
  `board::plugin::bad_input`.
- `comment --number <n> --body <b>` comments on a pull request or issue.
- `issue-create --title <t> --body <b>` opens an issue, writes its
  `github/issue/v1` record and returns `number` and `path`.

The destination is `github:<owner>/<repo>` in lower case, from the URL
configured for the checkout's `origin` in the https or ssh form. It is read
before any `insteadOf` rewrite. `insteadOf` moves fetch and push alike, so
the commits checked are still those the push sends. A `pushurl` or
`pushInsteadOf` that makes origin push somewhere other than it fetches is
refused as `github::remote::split`. No message quotes a URL's user or
password. The check names `from`, the checkout's mount
in the personal board, as `board refs .` reports it in the checkout. A
checkout no board mounts, or one mounted only in another board, is checked
without `from`, so every boundary on the destination applies. Run
`board --personal call github.<action>` from a checkout no board mounts.

The parts, which a boundary's `checks` name by glob:

| Part | Actions | Text |
| --- | --- | --- |
| `push.branch` | `push`, `pr-create` | The branch's name. |
| `commit.<sha>.message` | `push`, `pr-create` | The commit's message. |
| `commit.<sha>.author_name` | `push`, `pr-create` | The author's name. |
| `commit.<sha>.author_email` | `push`, `pr-create` | The author's email. |
| `commit.<sha>.committer_name` | `push`, `pr-create` | The committer's name. |
| `commit.<sha>.committer_email` | `push`, `pr-create` | The committer's email. |
| `commit.<sha>.diff` | `push`, `pr-create` | The commit's patch against its first parent, with every file as text: no textconv, external diff or `-diff` attribute hides a file's bytes. |
| `commit.<sha>.binary` | `push`, `pr-create` | The paths git counts as binary, one per line, so a boundary can refuse them by name; empty when there are none. |
| `pr.title`, `pr.body` | `pr-create`, `pr-edit` | The pull request's title and body. `pr-edit` sends only those given. |
| `comment.body` | `comment` | The comment. |
| `issue.title`, `issue.body` | `issue-create` | The issue's title and body. |

`<sha>` is the commit's first 12 hex digits, so `commit.*.message` names
every commit's message. A push sends the commits that no ref on origin
reaches, as `git ls-remote origin` lists them, so a new branch sends only
what the remote lacks.

Refusals:

- The boundary's refusal is `github::boundary::refused`. Board requires a
  plugin's codes to start with its own name, so `fields.code` holds the
  boundary's code, such as `boundary::check::failed`. The message and the
  other fields are the boundary's own, unchanged: `failures`, `digest`,
  `crossing` and `recovery`, whose `text` starts `Next:`. A code the
  boundary's refusal carried, such as `controls::control::not_found` inside
  `boundary::board::refused`, is in `cause`. Nothing is sent.
- Any other refusal of the check, such as boundary not being enabled in the
  personal board, is `github::board::refused` with Board's code.
- `github::checkout::not_found`: Board was not started in a git checkout.
- `github::remote::not_github`: origin is not a GitHub URL.
- `github::remote::split`: origin pushes to a URL other than the one it
  fetches from, or to several.
- `github::checkout::detached`: `push` or `pr-create` on no branch.
- `github::push::failed`: git refused the push, such as a branch that is
  behind. The crossing passed and keeps no result.
- `github::board::failed`: the plugin could not run `board`.
- `github::gh::failed`: gh failed after the check passed. The message says
  the content may have been sent.
- `github::git::failed`: another git command failed.

The sending actions receive gh's variables, `SSH_AUTH_SOCK` for git and the
variables the boundary's judged rules read. They pass the model credentials,
`ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN`, only to `board`, never to
git, its hooks or gh.
They may run for 2,400 seconds, since the check alone may take 1,800.

Owner and repository are stored in lower case, in the path and the fields,
because GitHub matches them without case; `url` keeps GitHub's own case. The
record keeps the path it was asked under, and `url` is where GitHub answered
from, which differs after a repository is renamed or an issue transferred.

`ref` passes gh `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` when Board has
them.

## Layout

Each capability is one module in `src/board_github/`, listed in
`__main__.py`; the sending actions share `send.py`. Every call to GitHub goes
through `gh.py`, every git command through `checkout.py`, and every call to the
boundary through `crossing.py`. `objects.py` holds the record types and their
paths.

## Installing

The sending actions need the boundary and controls plugins enabled in the
personal board.

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
reaches GitHub. The sending actions' tests run against a temporary personal
board named in a temporary `XDG_CONFIG_HOME`, with boundary and controls
enabled from this repository, so their `.venv`s must exist (`bin/lint python`
makes them). Their checkout's origin names GitHub, and its git config sends
every fetch and push to a local bare repository instead. `plugin.json` and the shipped schemas are generated from
`src/board_github/models.py` by `board_github.manifest`, and a test checks that
the committed files match.
