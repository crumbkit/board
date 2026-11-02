# inflight

`inflight` is built into Board (docs/plugins/built-in.md). It has one
read-only query that answers "what is open right now?". After a usage stop,
a new session runs it to find the work that was in flight. It writes
nothing and ships no schema.

## What it reads

- Changes from `changes.open`, and each change's gate and instruction from
  `changes.next`, through `board call`, so the step rules stay in the
  changes plugin (plugins/changes/README.md). Each change's `pull_request`
  URL is read from its record.
- Each change's artifacts from `artifacts.list --subject <change>`.
- Questions from `plans.open` and evals from `evals.open`.

## The query

    board --personal call inflight.now [--limit <n>]

It returns three parts. Each returns at most `limit` entries, 20 by default
and at most 50, and counts in `left_out` the entries the limit left out.
When the capability a part comes from refuses, as when its plugin is not
enabled, the part is empty and `refused` gives the capability, code and
message.

- `changes`: the open changes, by path. Each gives its `title`, `mount`,
  `phase`, `route`, `task` and `kind` from `changes.open`, its `gate` and
  `instruction` from `changes.next`, its `pull_request` URL, and its
  `artifacts`. `unreadable` counts the changes Board marks degraded.
- `questions`: the entries of `plans.open`, the questions waiting on the
  owner. `unreadable` counts the open questions it could not read.
- `evals`: the entries of `evals.open`, the evals planned or running.

When a call for one change refuses, its `refused` lists the capability,
code and message, and the other changes are still listed. `gate` and
`instruction` are then null when `changes.next` refused, and `artifacts` is
empty when `artifacts.list` did.

An artifact gives its `path`, `url`, `state`, and the `title` and `at` of
its newest version, newest first. Each change lists at most `limit`
artifacts and counts the rest in `artifacts_left_out`.

The query makes no network call, so it cannot say that a pull request
merged. A change with a `pull_request` gives `pr_state`, the command that
checks it: `gh pr view <url> --json state`.

`now` runs `changes.next` once per change it lists. While changes is a
Python plugin, each call starts a process that takes up to about a second
on a large board, so the capability has a timeout of 120 seconds and the
limit stops at 50.

## Not yet

Board does not yet record which session holds a change, the worktree it
works in, or the subagents a session starts. Until it does, `now` lists no
subagents and takes no `--holder` or `--stopped-at`. A session rescuing a
change starts from the change's `mount` and `instruction`.
