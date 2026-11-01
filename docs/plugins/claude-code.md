# Claude Code mods

This file holds detail for docs/plugins.md. It says what a plugin's Claude
Code mod is, and what `board-runs` writes and refuses.

A plugin may ship a mod in `plugins/<name>/claude-code/`. A mod is a Claude
Code plugin of function hooks: `.claude-plugin/plugin.json`,
`hooks/hooks.json` naming one TypeScript module, and the module itself. It
runs inside a Claude Code session and writes records only through the `board`
command, as an agent would. Board itself never runs a mod, and a mod has no
process of its own, so mods add no trigger or daemon to Board. Mods share data
only through Board records.

Board's own effects have no plugin, so their mod, `board-effects`, lives in
`plugins/effects/claude-code/`, where the `mods` workflow and the marketplace
find it. `plugins/effects/` holds nothing else and is not a Board plugin.
[effects-claude-code.md](effects-claude-code.md) says what it shows and runs.

The artifacts plugin's mod, `board-artifacts`, records every claude.ai
artifact a session publishes or reads; [claude-code/artifacts.md](claude-code/artifacts.md)
says what it writes.

Claude Code loads a mod with `claude --plugin-dir plugins/<name>/claude-code`,
or from the folders `CLAUDE_CODE_PLUGIN_DIRS` names. `claude plugin validate`
checks it and `claude plugin test` runs its `tests/*.test.ts`, which stand in
for `board` and `gh`. The `mods` workflow runs each mod's tests and
validates the marketplace below.
The release archive of a plugin does not include its mod.

The repository is also a Claude Code marketplace named `board`, listed in
`.claude-plugin/marketplace.json`. To install a mod, add the marketplace once
with `claude plugin marketplace add crumbkit/board`, then run
`claude plugin install <name>@board`, where `<name>` is the mod's name in its
`plugin.json`, such as `board-runs`, `board-controls`, `board-plans` or
`board-artifacts`. The `mods` workflow
fails when a mod has no entry in the marketplace.

## written_by

A record a hook writes without an agent carries `written_by`, the plugin and
hook that wrote it, such as `runs/subagent-stop`. It keeps the difference
between what an agent wrote and what was recorded for it. `runs/run/v1`,
`runs/agent/v1`, `evals/eval/v1` and `plans/answer/v1` take the field, and
a board takes it once `board plugin enable` has installed the plugin's schemas
again. A board whose schema lacks the field refuses the record with
`board::record::invalid`.

## board-runs

The runs plugin's mod, `plugins/runs/claude-code/`, records each subagent a
session starts and each unit it delegates, gives a subagent working on a
change its brief, and refuses `gh pr create` for a branch no open change
names. [claude-code/runs.md](claude-code/runs.md) says what it writes and
refuses.
