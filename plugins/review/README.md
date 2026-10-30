# review

A Board plugin for code review. The author of a change writes a brief.
`review.run` gives the brief and the diff to an agent that has not seen the
change, and records what it found as a report. `review.post` comments a report
on a pull request once the owner approves the comment. `review.covered` checks that a
committed report covers a pull request's commits, so CI can require a review
of each pull request.

## Records

`review/brief/v1`, written by the change's author at `/reviews/<id>/brief`:

- `range`: the commits under review as `base..head`, both full commit ids.
  Nothing else is allowed, so a range cannot pass git an option, and every git
  call also puts `--end-of-options` before revisions.
- `focus`: what matters and what to check first.
- `questions` (optional): what the author wants answered.
- `follows` (optional): the report this brief continues from.

`review/report/v1`, one review of a range, at `/reviews/<id>/report`:

- `range`: the full commit ids reviewed, `base..head`.
- `patch_ids`: the patch id of each commit in the range that has one, oldest
  first. They let the report cover the same commits after a rebase.
  - A patch id is `git diff-tree -p --no-renames --no-ext-diff --no-textconv
    --full-index <commit> | git patch-id --verbatim`, run without global or
    system git config.
  - `--verbatim` keeps whitespace, so a change of indentation alone is a
    different patch. It needs git 2.40 or later.
  - A merge or an empty commit has no patch id.
  - For a binary file git hashes the full blob ids, so two binary changes to
    one path get different ids.
- `findings`: each has `severity` (`high`, `medium` or `low`) and `text`, and
  `file` and `line` when it is about one place.
- `verdict`: `approve`, `changes` or `comment`.
- `by`: the models the agent reported, or the person. When the agent reports
  no model, it is the program the agent command starts.
- `brief`, `cost` (`seconds`, `input_tokens`, `output_tokens`), `source` (a
  URL such as a GitHub review) and `follows`: optional.

## The agent

`review.run` starts the argv in `BOARD_REVIEW_AGENT`, a JSON array of strings
the user sets. The first string names the program and must not be empty; an
argument may be empty:

```sh
export BOARD_REVIEW_AGENT='["claude", "-p", "--output-format", "json", "--setting-sources", "user", "--tools", "", "--strict-mcp-config"]'
```

- The command never comes from a board's tree. A pull request can change the
  tree, and it must not choose what runs on the reviewer's machine.
- No shell parses the command. It runs in an empty temporary directory, reads
  the prompt on stdin and prints its answer on stdout.
- The diff and the brief are untrusted, so run the agent with no tools, MCP
  servers or project settings. In the example, `--tools ""` loads no built-in
  tools, `--strict-mcp-config` loads no MCP servers, and
  `--setting-sources user` skips the project's settings. A verdict is no
  stronger than the agent's resistance to text in the diff.
- The answer must end with a JSON object holding `verdict` and `findings`.
- When the output is the JSON envelope of `claude -p --output-format json`,
  `review.run` reads the answer from `result`, the tokens from `usage` and the
  models from `modelUsage`.

## Capabilities

`board call review.run --brief /reviews/<id>/brief`:

- Reads the brief from the board's own tree.
- Resolves the brief's range with git, and replaces the base with the merge
  base of the base and the head, so commits the base gained after the branch
  point do not show as reverts.
- Starts the agent with a prompt holding the brief and the diff. It has 30
  minutes, and sees `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` when Board
  has them.
- Tells the agent that it has no tools, so the diff is all it has.
- Stops the agent, with every process it started, when it exits, after 29
  minutes with `review::agent::timeout`, or when the plugin gets SIGTERM,
  SIGINT or SIGHUP. After a timeout it reads the agent's output for at most
  5 seconds more, in case a process that left the group still holds it.
- Writes the report at `/reviews/<id>/report` with `board put`, and returns
  its path, the range, the verdict and the number of findings.
- Fails with `review::report::exists` when the review already has a report;
  write a new brief whose `follows` names it.
- Fails with `review::brief::not_found`, `review::agent::not_set`,
  `review::agent::invalid`, `review::range::unknown`,
  `review::agent::not_started`, `review::agent::failed` (with the exit code
  and the ends of stdout and stderr), `review::agent::timeout` or
  `review::agent::bad_output`.

`board call review.post --report /reviews/<id>/report --pr <url>`:

- `--pr` is the pull request's full URL, `https://github.com/<owner>/<repo>/pull/<n>`.
- Reads the report from the board's own tree. A report that is not there,
  or that its schema refuses, fails with `review::report::not_found` before
  anything is asked.
- Skips a report whose `source` is set, without asking or posting, and
  returns that URL as `comment` with `posted: false`. A posted report, or
  one recorded from a GitHub review, has one. A report is posted once, to
  the pull request its `source` names.
- Builds the comment from the report: the verdict, `by`, the range, each
  finding with its severity, file and line, and the report it follows. A
  footer names the report and the pull request. Every `@` is followed by a
  zero-width joiner, so text an agent wrote from an untrusted diff mentions
  no one.
- Asks the owner with `board decide --op review.post --path <report> --text -`,
  showing the whole comment. Until the owner approves, it fails with
  `review::board::refused` carrying Board's `board::settings::asked` or
  `board::settings::refused`, its message and the `effect` id. The effect's
  id covers the text, and the footer puts the pull request in the text, so
  an approval admits only that comment on that pull request. A changed
  report, or another pull request, asks again.
- Once approved, runs `gh pr comment <url> --body-file -` with the comment on
  stdin, then sets the report's `source` to the URL gh printed, and returns
  it as `comment` with `posted: true`. gh reads its login from its own
  config, or `GH_TOKEN`, `GITHUB_TOKEN` or `GH_CONFIG_DIR` when Board has
  them. Commit the report afterwards; `review.covered` skips a commit that
  only sets, changes or removes a report's `source`.
- The approval stays, so a re-run of a report that has no `source` posts
  again without asking.
- Fails with `review::gh::failed` when gh cannot start, runs past 40
  seconds, exits non-zero, or prints something other than a comment URL on
  the pull request. The message carries gh's stderr. The report is
  unchanged. gh may still have posted the comment, so check the pull
  request for it before posting again.
- If gh posted but the report could not be written, posting again posts a
  second comment.

`board call review.covered --base <commit> [--head <commit>]`:

- Checks each commit from the merge base of `--base` and the head to the head.
  `--base` is required, so CI cannot check the tip alone. The head is HEAD by
  default.
- A commit that only adds briefs and reports, or only sets, changes or
  removes a report's `source`, needs no report, so committing one leaves the
  head covered. Each added file must be a `review/brief/v1` at
  `reviews/<id>/brief.json` or a `review/report/v1` at
  `reviews/<id>/report.json`. A merge, a commit that adds any other file,
  changes any other field or deletes a file always needs one.
- A report with the verdict `changes` never covers.
- Another report covers each commit its range contains, and each commit whose
  patch id it lists, as after a rebase. A merge or an empty commit has no
  patch id, so only a range that contains it covers it. Several reports may
  together cover the commits.
- A report covers by patch id only when every patch id it lists belongs to a
  commit in the range. A review of two commits, where the second undoes part
  of the first, then does not cover the first alone. This refines
  plan/review.md.
- Only reports committed at HEAD in the board's own tree count. A report
  changed on disk, or reached through a mount, is ignored.
- Returns the head, the merge base, the number of commits skipped and checked,
  and the reports that cover them.
- Fails with `review::head::uncovered`, which lists the first uncovered
  commits, `review::range::unknown` for a commit git cannot resolve, or
  `review::range::unrelated` when the base and head share no commit.

Coverage proves that reports are committed. It does not prove that
`review.run` wrote them: anyone who can add a commit to the pull request,
from a fork too, can commit an approving report, and the workflow that runs
the check is part of the pull request. The check is evidence for the owner,
not a merge gate.

In a GitHub pull request, HEAD is a merge of the branch into the base. Check
out the full history and pass the branch's base and head to the check:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- run: >
    board call review.covered --base "${{ github.event.pull_request.base.sha }}"
    --head "${{ github.event.pull_request.head.sha }}"
```

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/review"
    board plugin enable review

Then set `BOARD_REVIEW_AGENT`. The types stay at v1 and change in place while
the plugin has one user; after such a change, run `board plugin enable review`
again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. The tests start a stub agent script, never a
model, and a stub gh, never GitHub. An approval needs the owner's
signature, so the approved tests put a `board` shim on PATH that runs the
plugin's decide as the owner, whom Board admits. `plugin.json` and the shipped schemas are generated from the shapes in
`src/board_review/shapes.py` by `board_review.manifest`, and a test checks that
the committed files match.
