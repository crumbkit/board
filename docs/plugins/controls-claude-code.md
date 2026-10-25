# The controls mod for Claude Code

This file holds detail for docs/plugins.md. It says why the Claude Code mod in
`plugins/controls/claude-code/` tells a session about controls the way it
does. `plugins/controls/README.md` says how to install it.

A session that edits a file a control governs should know the control before
the edit. The controls skill asks the agent to run `controls.check` itself,
and agents forget. The mod runs the check for them and reads nothing else from
Board, so it needs no state of its own beyond the session.

## At session start

When a session starts, the mod finds the repository root with
`git rev-parse --show-toplevel` and runs
`board --json call controls.check --path .` there. Board maps the directory to
the board that holds or mounts it. The answer goes into one `prompt.context`
block, which the engine sends in the first request and again after a
compaction. A repository that no board holds gives `board::config::not_found`,
and the mod then adds nothing. Any other failure is said in the block, with
the command to run by hand.

The mod shows one line, such as "2 controls on crumbkit". When the attention
mod is loaded, which the mod learns from its
`mcp__attention-approve__request_signature` tool, the
line joins the band above the prompt that attention draws. Otherwise it goes
to the status line.

## Before an edit

A hook on `Edit` and `Write` runs `controls.for-path` at the repository root,
with the file's path relative to it, before the edit runs. A file outside the
repository, or one in a repository no board holds, runs nothing. When the answer names a control the conversation was not told
about, the hook appends a hidden row that names it, so the model reads it
before its next step. Controls from the session-start check count as told.
Each agent, the main conversation and every subagent, has its own list. A
compaction or `/clear` drops the hidden rows, so the main conversation's list
goes back to the controls in the block.

The answer is cached per file for the session. A cache per directory was
rejected because a pattern such as `*.rs` can match one file in a directory
and not another. Each board run stops after 5 seconds. A lookup that fails or
stops lets the edit run as it would without the mod.

## Testing

`claude plugin test plugins/controls/claude-code` runs the mod's tests, which
stand in for git and Board. CI does not run them yet. The Python tests prove
what `controls.check` and `controls.for-path` answer.
