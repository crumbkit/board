# The handoff mod

This file holds detail for docs/plugins/claude-code.md. It says when
`board-handoff` tells a session to hand off and what it writes.

`board-handoff`, in `plugins/handoff/claude-code/`, tells a session to hand
off before it runs out of context or account usage. It is a mod and no Board
plugin, so it adds no capability.

After each turn Claude Code gives a mod the session's context share and the
account's rate-limit windows (`session.measure`). When the context share
reaches `handoff-context-share`, or a window reaches `handoff-usage-share`,
the mod submits one prompt that tells the session to follow the handoff skill
now. Both values are read once per session with `board --personal call
params.get`; without a value the mod uses 75 and 85. For a usage window the mod
also writes `trigger.json` in `board-handoff/` under the account's Claude
config directory. Every session on that account checks the file every 30
seconds and gets the same prompt, once, until the window resets.

When the told turn ends, the session shows `handoff: cd <dir> && claude
--resume <id>  # <its handoff line>` and keeps it in `sessions/<id>.json`.
`/handoff-lines` prints every session's line. The mod does not end a session
and cannot make the model obey the prompt.

