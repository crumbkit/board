# The rules mod for Claude Code

This file holds detail for docs/plugins.md. It says why the Claude Code mod in
`plugins/rules/claude-code/` tells a session about rules the way it
does. `plugins/rules/README.md` says how to install it.

A session that edits a file a rule governs should know the rule before
the edit. The rules skill asks the agent to run `rules.check` itself,
and agents forget. The mod runs the check for them and reads nothing else from
Board, so it needs no state of its own beyond the session.

## At session start

When a session starts, the mod finds the repository root with
`git rev-parse --show-toplevel` and runs
`board --json call rules.check --path .` there. Board maps the directory to
the board that holds or mounts it. The answer goes into one `prompt.context`
block, which the engine sends in the first request and again after a
compaction. A repository that no board holds gives `board::config::not_found`,
and the mod then adds nothing. Any other failure is said in the block, with
the command to run by hand. A budget over its max, a pin behind or an unreadable file is
data in a passing answer, and the block says so beside the rule, such as
"now 300 of 200 words, over budget".

The mod's badges in the band above the prompt are described in
[claude-code/band.md](claude-code/band.md).

## Before an edit

A hook on `Edit` and `Write` runs `rules.for-path` at the repository root,
with the file's path relative to it, before the edit runs. A file outside the
repository, or one in a repository no board holds, runs nothing. When the answer names a rule the conversation was not told
about, the hook appends a hidden row that names it, so the model reads it
before its next step. Rules from the session-start check count as told.
Each agent, the main conversation and every subagent, has its own list. A
compaction or `/clear` drops the hidden rows, so the main conversation's list
goes back to the rules in the block.

The answer is cached per file for the session. A cache per directory was
rejected because a pattern such as `*.rs` can match one file in a directory
and not another. Each board run stops after 5 seconds. A lookup that fails or
stops lets the edit run as it would without the mod.

## Testing

`claude plugin test plugins/rules/claude-code` runs the mod's tests, which
stand in for git and Board. CI does not run them yet. The tests in
`crates/board-cli/src/tests/built_in/rules.rs` prove what `rules.check` and `rules.for-path` answer.
