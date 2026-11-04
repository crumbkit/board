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
  - gh missing, failing, running past 20 seconds, printing more than 16 MiB
    or answering without an `html_url` is `github::gh::failed`, with gh's error or the command that
    shows the answer in the message.
  - A record already there that Board marks degraded, such as one with
    capitals in `owner`, is `github::record::degraded`, with Board's code in
    `fields.code`. A record whose body does not read as a JSON object is the
    same code with `github::record::unreadable` in `fields.code`.

### Sending actions

`push`, `pr-create`, `pr-edit`, `comment` and `issue-create` send content
from the checkout Board was started in, which Board names in
`BOARD_CALLER_DIR`. Each action works out where the content goes, collects
it as named parts, and calls `boundary.check` in the personal board on them
as the action `github.<name>`. It sends only when the check passes. Then it
records what GitHub returned with `boundary.record`, and returns `digest`, `crossing`, `note` and `sent`, the
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
by one rule, `github_repository` in `remote.rs`. The host is matched
without case, with any port, without the brackets git strips and without a
trailing dot. `github.com`, `www.github.com` and `ssh.github.com` count as
GitHub, and so does `github.com-<alias>` over ssh. An ssh host with another
name is resolved through `ssh -G`, so a `Host` alias whose `HostName` is
github.com counts too. Only a plain host name, letters, digits, `.`, `_` and
`-`, is passed to ssh. When git runs another ssh command, through
`core.sshCommand`, `GIT_SSH_COMMAND` or `GIT_SSH`, that command may resolve
hosts differently, so an ssh host that is not plainly GitHub's is
`github::remote::unknown`. A local path, a `file://` URL and
another host are not GitHub, which the sending actions refuse as
`github::remote::not_github`. A URL that could reach GitHub but names no
repository the rule reads, such as a `<transport>::<address>` remote helper,
an ssh host that `ssh -G` cannot resolve or that is not a plain name, or a
GitHub URL with no repository, is `github::remote::unknown`. `insteadOf` moves fetch and push alike, so
the commits checked are still those the push sends. A `pushurl` or
`pushInsteadOf` that makes origin push somewhere other than it fetches is
refused as `github::remote::split`. No message quotes a URL's user or
password. The check names `from`, the checkout's mount
in the personal board: the one place where a mount of the personal board
holds the checkout's repository. A checkout the personal board does not
mount, or mounts more than once, is checked without `from`, so every
boundary on the destination applies. Run
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
| `commit.<sha>.diff` | `push`, `pr-create` | The commit's patch against its first parent, with every file as text: no textconv, external diff or `-diff` attribute hides a file's bytes, and file names are not quoted. A text file's bytes, a carriage return included, are those sent. In a binary file, each byte that is not UTF-8 is checked as U+FFFD. |
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
  boundary's code, such as `boundary::check::failed`. The message is the
  boundary's, followed by its `Next:` step, which is also the hint. The
  other fields are the boundary's own, unchanged: `failures`, `digest`,
  `crossing` and `effect`. A code the
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
- `github::boundary::unreadable`: the boundary answered in a shape this
  plugin does not read, which happens only when the two come from
  different Board builds.
- `github::gh::failed`: gh failed after the check passed. The message says
  the content may have been sent.
- A refusal after GitHub took the content, such as a record that cannot be
  written or a crossing that cannot take its result, keeps its own code but
  starts its message by saying the content was already sent and must not be
  sent again. `fields.sent` names the pull request's, issue's or comment's
  URL, or git's line for a push. When `pr-create` fails after its push but
  before gh runs, the message says the commits were pushed but the pull
  request was not opened, and that running `pr-create` again opens it.
- `github::remote::unknown`: a URL could reach GitHub, but the plugin cannot
  tell which repository it names.
- `github::push::not_commit`: the pre-push hook found a ref whose object is a
  tree or a blob, which it cannot check as commits.
- `github::git::not_utf8`: a commit changes a file git counts as text whose
  bytes are not UTF-8, or a commit message, name or tag git prints for a
  part is not UTF-8. The boundary checks text, so it could not check the
  bytes sent, and nothing is pushed. The fields name the commit, or the git
  command.
- `github::git::failed`: another git command failed.
- `github::hook::exists`: `install-hook` found a different `pre-push` hook,
  which its command would replace.
- `github::hook::shared`: `core.hooksPath` comes from the global or system
  git config, so the hook would serve every repository.

The sending actions receive gh's variables, `SSH_AUTH_SOCK` for git and the
variables the boundary's judged rules read. They pass the model credentials,
`ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN`, only to the boundary's
check, never to git, its hooks, ssh or gh. They pass gh's tokens, `GH_TOKEN`
and `GITHUB_TOKEN`, only to gh, never to git, its hooks or ssh, so no hook
in the checkout can read them. `GH_CONFIG_DIR` still reaches git, so
`gh auth git-credential` finds gh's stored sign-in for an HTTPS push. So
git needs a stored sign-in from `gh auth login` and `gh auth setup-git`, or
an SSH remote; a token in `GH_TOKEN` or `GITHUB_TOKEN` alone signs in gh
and not git. A git command that fails to sign in while a token variable is
set, such as the `git ls-remote` before the check or the push itself, is
`github::git::failed` or `github::push::failed` with a hint saying so. The
pre-push capability is not given the tokens, so in the hook any git
command that fails to sign in carries the hint. Every git and ssh command
runs through the host with a deadline, 300 seconds for git and 10 for `ssh -G`,
and its output is capped at 16 MiB, so a larger diff fails with
`github::git::failed`.
Git runs the checkout's `pre-push` hook during the push, as it does for any
push. The plugin does not check what a hook sends. When the hook is this
plugin's own, it reuses the check the action passed, as below.

The sending actions may run for 2,400 seconds, since the check alone may take 1,800.

Owner and repository are stored in lower case, in the path and the fields,
because GitHub matches them without case; `url` keeps GitHub's own case. The
record keeps the path it was asked under, and `url` is where GitHub answered
from, which differs after a repository is renamed or an issue transferred.

`ref` declares `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` as its `env`,
so gh sees them when Board has them, with the variables any plugin's process
sees.

## Pushes and writes made without Board

The sending actions check only what goes through them. Two layers catch a
push or write made another way. GitHub's own controls, such as branch
protection and push rules, stay in place behind both, as the last layer.

### The git pre-push hook

`install-hook` writes nothing, since Board has no path that writes a file
outside a board's tree. It returns `path`, where the hook goes in the
checkout Board was started in, in the directory `git rev-parse --git-path
hooks` names, so it honours a `core.hooksPath` the repository sets and serves
every worktree of the repository; `current`, true when the same hook is
already there; and `command`, the shell command that makes that directory,
writes the hook there, its text in a here-document, and makes it
executable. A hook whose marker line says this plugin installed it is
replaced by the command. Any other hook at that path is
`github::hook::exists`. A `core.hooksPath` from the global or system git
config would serve every repository on the machine, so it is
`github::hook::shared`.

Before each push, git runs the hook. The hook first runs
`board --json --personal refs /`. When the `code` field of Board's failure
document is `board::config::not_found`, no config names a personal board,
so there is no boundary at all, and the push goes. That output holds nothing
of the push, so no text in a commit can match it. Otherwise the hook runs
`board --personal call github.pre-push` with the remote and the refs git
gives it, and the URL git pushes to in `BOARD_GITHUB_PUSH_URL`, so the URL
and any password in it stay off the command line.

For each ref, `pre-push` builds the parts `push` builds. `push.branch` is the
remote ref without `refs/heads/`, so a tag is `refs/tags/<name>`. The commits
are those no ref at the URL git pushes to reaches, so a push URL unlike the
fetch URL is compared with what the push URL holds. The hook lists that
URL's refs once per push, however many refs it pushes. An annotated tag adds
`tag.<sha>.message`, `tag.<sha>.tagger_name`, `tag.<sha>.tagger_email` and
`tag.<sha>.headers` for each tag down to the commit, with `<sha>` the tag
object's first 12 hex digits. `headers` holds the tag's header lines other
than `object`, `type` and `tagger`, so it holds the tag's own name, which can
differ from the ref it is pushed to, and the name of a tag nested in another.
A ref whose object is a tree or a blob is `github::push::not_commit`.
A ref git deletes sends nothing and is not checked. It runs `boundary.check`
on the parts as the action `github.push`. A push made by `push` or
`pr-create` has already passed the boundary, so the action hands the hook
that check in `BOARD_GITHUB_PASSED`: the crossing's digest, its action, and
the parts it checked beyond the ref's, the pull request's title and body.
The hook reuses that check only when all of these hold:

- the action is `github.push` or `github.pr-create`;
- the handed parts are only those the action adds, `pr.title` and
  `pr.body` for `pr-create` and none for `push`, and none replaces a part
  the hook built from the ref;
- `boundary.verify` finds a passing crossing of that digest for the ref's
  parts with the handed ones;
- every boundary that `boundary.covers` names for `github.push` to that
  destination, from the hook's own checkout, is among the boundaries that
  crossing was checked against.

Otherwise the hook runs `boundary.check` on the ref's parts. A handed value
stands for a check that applied every boundary this push meets to exactly
the content it sends, under the rules in force when that check ran. The
hook then asks no agent again, even when `BOARD_CONTROLS_AGENT` names no
`--model` and the hook, like every git hook, runs without the model
credentials. A raw `git push` hands nothing and is checked. It returns
`checked`, each ref's `ref`, `to`, `digest` and `note`.

The destinations are every GitHub repository among the URL git pushes to
and the remote's configured push URLs, else its URLs, so a push rewritten to
a local path, as in the tests, is still checked against the repository the
person wrote. A URL that is `github::remote::unknown` refuses the push. A push
to no GitHub repository is not checked.

The hook refuses the push whenever the check fails or cannot run, with
Board's message, which ends with the boundary's `Next:` step when it gives
one. It refuses
when board is not on PATH, and when the boundary plugin is not enabled,
because it cannot then tell whether a boundary covers the push. A push no
boundary covers passes the check with a `note` and goes.

The hook guards against mistakes, not against an agent that sets out to
get round it. `github.push` and `pr-create` check again on every run, so
only a raw `git push` meets these ways round the hook, which is why
GitHub's controls stay:

- `git push --no-verify`, `git -c core.hooksPath=<elsewhere> push` and an
  alias that sets it.
- Submodule commits that `push.recurseSubmodules` pushes through the
  submodule's own hooks directory, where no hook is installed.
- A hand-set `BOARD_GITHUB_PASSED` that replays an earlier passing check
  of the same content. `boundary.verify` compares only the boundaries'
  paths, so the replay passes after a control's rule, a pattern file or a
  boundary's checks has been tightened, at any time after that check ran.
- Pointing Board's config at an empty board, or at none: the hook lets the
  push go on `board::config::not_found`.

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
  else origin. A push goes to the remote's push URLs, else its URLs, and a gh
  write to the remote's URLs, since gh reads the fetch URL. It asks
  `boundary.covers` with the checkout's mount, as the sending actions do. A `--repo` on a host other than github.com is not
  read. gh may pick another repository, such as a fork's parent, and the mod
  does not follow it.
- `gh api graphql` is not refused, because a mutation names its repository
  by node id, which the text does not show.
- `covers` reads the git config and board mount of the directory the
  command names before the person approves the command. No git hook runs.
- The run stops after 5 seconds, and the call then runs.

`covers --action <action> [--remote <remote>] [--repo <owner>/<repo>]`
returns `covered`, each destination a boundary covers, with `to` and
`boundaries`. The action is `push`, `pr-create`, `pr-edit`, `comment`,
`issue-create`, or `api` for a gh write Board has no action for, which a
boundary matches as `github.api`; any other is refused as bad input. It
checks nothing and writes nothing.

Install it with `claude plugin install board-github@board`, after
`claude plugin marketplace add crumbkit/board`. `claude plugin test
plugins/github/claude-code` runs its tests, which stand in for board and git.

## Layout

The plugin is built into Board as the crate `crates/board-plugin-github`.
Each capability is listed once in `CAPABILITIES` in `lib.rs`. `ref` is in
`reference.rs`, the sending actions in `send.rs`, and `install-hook`,
`pre-push` and `covers` in `hook.rs`. Every call to GitHub goes through
`gh.rs`, every git command through `git.rs`, the parts a push sends are
built in `parts.rs`, the URL rule is in `remote.rs`, and every call to the
boundary goes through `crossing.rs`. `objects.rs` holds the record kinds,
their paths and their writing, and `records.rs` the record types.

## Installing

It is built into Board. `board plugin enable github` writes the schemas to
`.board/types/github/` and names the plugin in `.board/board.json`. The
sending actions need the boundary and controls plugins enabled in the
personal board. An older install at
`${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/github` makes `call` and
`plugin enable` fail with `board::plugin::duplicate`. Remove it; it is
usually a symlink to a checkout, so remove the link, not the checkout.

## Developing

The shipped schemas are generated from `src/records.rs`, and the files under
the crate's `types/` keep the bytes the Python plugin shipped. A test checks
that they match; `SNAPSHOTS=overwrite` rewrites them. The URL rule and the
parsing of git's output are tested in the crate. The tests of the plugin
through the binary are in `crates/board-cli/src/tests/built_in/github.rs`
and the files beside it. They put a stub gh first on the PATH the plugin's
process sees, so no test reaches GitHub. The sending actions' tests run
against a temporary personal board named in a temporary known boards file,
with boundary and controls enabled. Their checkout's origin names GitHub,
and its git config sends every fetch and push to a local bare repository
instead. The hook tests install the hook with the command `install-hook`
prints and push with git, which runs the `board` cargo built.
