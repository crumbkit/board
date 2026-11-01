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
  commits and no push options, whatever git's config says. It records git's
  line on the crossing, then sets the branch's upstream in git's config as
  `git push -u` does, which works in a single-branch clone too. It returns `branch`, `commits`, the commits sent, oldest
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
configured for the checkout's `origin`. It is read before any `insteadOf`
rewrite. The sending actions, the pre-push hook and `covers` read every URL
by one rule, `github_repository` in `checkout.py`. The host is matched
without case and with any port, and `github.com`, `www.github.com`,
`ssh.github.com` and `github.com-<alias>` count as GitHub. An ssh host with
another name is resolved through `ssh -G`, so a `Host` alias whose
`HostName` is github.com counts too. A local path, a `file://` URL and
another host are not GitHub, which the sending actions refuse as
`github::remote::not_github`. A URL that could reach GitHub but names no
repository the rule reads, such as a `<transport>::<address>` remote helper,
an ssh host that `ssh -G` cannot resolve, or a GitHub URL with no
repository, is `github::remote::unknown`. `insteadOf` moves fetch and push alike, so
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
| `commit.<sha>.headers` | `push`, `pr-create` | The commit object's header lines other than `tree`, `parent`, `author` and `committer`, such as `encoding` or a merged signed tag's `mergetag`; empty when there are none. |
| `commit.<sha>.diff` | `push`, `pr-create` | The commit's patch against its first parent, with every file as text: no textconv, external diff or `-diff` attribute hides a file's bytes, and file names are not quoted. |
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
- `github::push::lfs`: a commit changes a file whose `filter` attribute is
  `lfs`. Git LFS's `pre-push` hook uploads such a file's bytes, which the
  boundary cannot see, so nothing is pushed. The fields name the commit and
  paths.
- `github::push::failed`: git refused the push, such as a branch that is
  behind. The crossing passed and keeps no result.
- `github::board::failed`: the plugin could not run `board`.
- `github::gh::failed`: gh failed after the check passed. The message says
  the content may have been sent.
- `github::remote::unknown`: a URL could reach GitHub, but the plugin cannot
  tell which repository it names.
- `github::push::not_commit`: the pre-push hook found a ref whose object is a
  tree or a blob, which it cannot check as commits.
- `github::git::failed`: another git command failed.
- `github::hook::exists`: `install-hook` found a different `pre-push` hook.
- `github::hook::shared`: `core.hooksPath` comes from the global or system
  git config, so the hook would serve every repository.

The sending actions receive gh's variables, `SSH_AUTH_SOCK` for git and the
variables the boundary's judged rules read. They pass the model credentials,
`ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN`, only to `board`, never to
git, its hooks or gh.
Git runs the checkout's `pre-push` hook during the push, as it does for any
push. The plugin does not check what a hook sends. When the hook is this
plugin's own, it checks the same parts again, as below.

The sending actions may run for 2,400 seconds, since the check alone may take 1,800.

Owner and repository are stored in lower case, in the path and the fields,
because GitHub matches them without case; `url` keeps GitHub's own case. The
record keeps the path it was asked under, and `url` is where GitHub answered
from, which differs after a repository is renamed or an issue transferred.

`ref` passes gh `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` when Board has
them.

## Pushes and writes made without Board

The sending actions check only what goes through them. Two layers catch a
push or write made another way. GitHub's own controls, such as branch
protection and push rules, stay in place behind both, as the last layer.

### The git pre-push hook

`install-hook` writes a `pre-push` hook in the checkout Board was started in,
in the directory `git rev-parse --git-path hooks` names, so it honours a
`core.hooksPath` the repository sets and serves every worktree of the
repository. It returns `path` and `installed`, which is false when the same
hook was already there. A hook whose marker line says this plugin installed
it is replaced by the current one. Any other hook at that path is
`github::hook::exists`, and the file is left as it was. A `core.hooksPath`
from the global or system git config would serve every repository on the
machine, so it is `github::hook::shared`, and nothing is written.

Before each push, git runs the hook. The hook first runs
`board --json --personal refs /`. When that prints Board's failure document
with the code `board::config::not_found`, no config names a personal board,
so there is no boundary at all, and the push goes. That output holds nothing
of the push, so no text in a commit can match it. Otherwise the hook runs
`board --personal call github.pre-push` with the remote and the refs git
gives it, and the URL git pushes to in `BOARD_GITHUB_PUSH_URL`, so the URL
and any password in it stay off the command line.

For each ref, `pre-push` builds the parts `push` builds. `push.branch` is the
remote ref without `refs/heads/`, so a tag is `refs/tags/<name>`. The commits
are those no ref at the URL git pushes to reaches, so a push URL unlike the
fetch URL is compared with what the push URL holds. An annotated tag adds
`tag.<sha>.message`, `tag.<sha>.tagger_name` and `tag.<sha>.tagger_email`
for each tag down to the commit, with `<sha>` the tag object's first 12 hex
digits. A ref whose object is a tree or a blob is `github::push::not_commit`.
A ref git deletes sends nothing and is not checked. It runs `boundary.check`
on the parts as the action `github.push`. It never relies on
`boundary.verify`, which misses an edit to a boundary's checks. A push made
by `push` or `pr-create` gives the hook the same content, so judged verdicts
come from the controls plugin's cache and cost no second model call. It
returns `checked`, each ref's `ref`, `to`, `digest` and `note`.

The destinations are every GitHub repository among the URL git pushes to
and the remote's configured push URLs, else its URLs, so a push rewritten to
a local path, as in the tests, is still checked against the repository the
person wrote. A URL that is `github::remote::unknown` refuses the push. A push
to no GitHub repository is not checked.

The hook refuses the push whenever the check fails or cannot run, with
Board's message, which ends with the boundary's `Next:` step. It refuses
when board is not on PATH, and when the boundary plugin is not enabled,
because it cannot then tell whether a boundary covers the push. A push no
boundary covers passes the check with a `note` and goes. Some pushes skip
the hook, which is why GitHub's controls stay: `git push --no-verify`,
`git -c core.hooksPath=<elsewhere> push` and an alias that sets it, and
submodule commits that `push.recurseSubmodules` pushes through the
submodule's own hooks directory, where no hook is installed.

### The Claude Code mod

The mod in `claude-code/`, `board-github`, adds a hook on Bash calls. It
refuses `git push`, `gh pr create`, `gh pr edit`, `gh pr comment`,
`gh issue create`, `gh issue comment` and `gh api` calls that write to a
repository, when a boundary in the personal board covers the destination.
It also refuses these commands when they carry text by flag, with no
matching action: `gh pr review`, `gh pr merge`, `gh pr close`,
`gh pr reopen`, `gh issue close`, `gh issue reopen`, `gh issue edit`,
`gh release create`, `gh release edit` and `gh repo edit`. It does not read
other gh commands, such as `gh repo create --push`, `gh gist create` or
`gh workflow run`.
The refusal names the github action that sends the same content, with the
title, body, base and number the command gave, such as
`Next: run \`board call github.pr-create --title "Add a gear" --body "..."\``.
The command name is the constant `BOARD_COMMAND` in `claude-code/hooks/command.ts`.

The mod refuses only when it is sure. It must be sure the command writes to
GitHub, and sure a boundary covers that destination for that action.
Whenever it cannot tell, the call runs and the mod says nothing. It cannot
tell when board is missing or fails, when the personal board cannot be read,
when the command does not parse, such as an unclosed quote, or when a
directory or remote the command names is expanded by the shell, such as
`cd "$DIR"`, or another git directory such as `--git-dir` or `GIT_DIR`. A
wrong refusal costs the agent a working command and teaches it to route
around the mod, so the mod leans towards letting a call run. A missed push
still meets the pre-push hook where it is installed, and GitHub's controls.
Missed pull request, issue or comment text has no later check.

How it reads a command:

- It reads only the text. A command that names neither `git` nor `gh` runs
  with nothing read. A read, such as `gh pr view` or a `gh api` call with no
  method and no fields, runs with nothing read either.
- It splits the text at `;`, `&&`, `||`, `|`, `&`, parentheses and line ends,
  skips here-document bodies, and drops redirections, leading variable
  assignments and `env`, `command`, `exec`, `time` and `nohup`. `cd` and
  `git -C` move the commands after them, up to the end of a subshell they
  are in, and the refusal names the same directory. A here-document inside
  `$(...)` is literal text, so a quote in it opens nothing. A command inside
  `$(...)` is not read.
- gh flags are read as each command takes them: `gh api` takes a field after
  `-f`, and a short flag's value may be joined to it, as in `-XPOST`.
- For each write, it runs `board --json --personal call github.covers` in
  the directory, with the action and the repository `--repo` or a `gh api`
  path names, or the remote a `git push` names. `covers` finds the
  destinations by the URL rule above, else from the branch's push remote,
  else origin, and asks `boundary.covers` with the checkout's mount, as the
  sending actions do. A `--repo` on a host other than github.com is not
  read. gh may pick another repository, such as a fork's parent, and the mod
  does not follow it.
- `gh api graphql` is not refused, because a mutation names its repository
  by node id, which the text does not show.
- `covers` reads the git config and board mount of the directory the
  command names before the person approves the command. No git hook runs.
- The run stops after 5 seconds, and the call then runs.

`covers --action <action> [--remote <remote>] [--repo <owner>/<repo>]`
returns `covered`, each destination a boundary covers, with `to` and
`boundaries`. It checks nothing and writes nothing.

Install it with `claude plugin install board-github@board`, after
`claude plugin marketplace add crumbkit/board`. `claude plugin test
plugins/github/claude-code` runs its tests, which stand in for board and git.

## Layout

Each capability is one module in `src/board_github/`, listed in
`__main__.py`; the sending actions share `send.py`, and `hook.py` holds `install-hook`,
`pre-push` and `covers`. Every call to GitHub goes
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
