# The handoff mod

This file holds detail for docs/plugins/claude-code.md. It says when
`board-handoff` tells a session to hand off and what it writes.

`board-handoff`, in `plugins/handoff/claude-code/`, tells a session to hand
off before it runs out of context or account usage. It is a mod and no Board
plugin, so it adds no capability.

## The context limit

After each turn Claude Code gives a mod the session's context and the
account's rate-limit windows (`session.measure`). The mod counts the tokens
in the session's context from `context.tokens` in that payload. When the
payload has no count, the mod reads the transcript that the last `Stop` hook
input named in `transcript_path`, and adds `input_tokens`,
`cache_read_input_tokens` and `cache_creation_input_tokens` of its last
assistant entry. When neither gives a count, the mod skips the context check
for that turn, logs once per session, and still checks usage.

When the count reaches the param `handoff-context-tokens`, the mod submits one
prompt that names the count and the limit and tells the session to follow the
handoff skill now. The session gets this prompt even when it was told about
account usage earlier. A count back under the limit, as after `/compact`,
re-arms the prompt. Without a value in the board the limit is 280,000 tokens.

The limit is a token count, so it does not depend on the size of the window
the model reports. Version 0.1.1 compared a share of that window with
`handoff-context-share` instead, and a 1,000,000-token window kept that share
from being reached. Version 0.1.2 no longer reads `handoff-context-share`.

To change the limit, write a new default to the heuristic:

```bash
board -C ~/Code/board get /params/heuristics/handoff-context-tokens \
  | jq '.default = 300000' \
  | board -C ~/Code/board put /params/heuristics/handoff-context-tokens
```

`board -C ~/Code/board call params.get --name handoff-context-tokens` prints
the value in force. The mod reads `handoff-context-tokens` and
`handoff-usage-share` with `board --personal call params.get` when a session
first measures, and again on the first measurement after its copy is over 5
minutes old. So a changed value reaches running sessions within 5 minutes,
without a restart. One `params.get` call takes about 0.36 seconds, which is
too slow to run after every turn. When `board` is missing or refuses, the mod
uses 280,000 tokens and 85% usage and logs that once.

## Account usage

When a rate-limit window reaches `handoff-usage-share`, the mod submits the
same kind of prompt, once, and writes `trigger.json` in `board-handoff/` under
the account's Claude config directory. Every session on that account checks
the file every 30 seconds. A session whose own latest reading of that window
is under `handoff-usage-share` is not told, because the trigger is stale for
it. Its next reading at or over the share tells it and writes a fresh
trigger. A session that has no reading of that window yet is told, once,
until the window resets.

A usage reset lowers a window's share but keeps its reset time. So when a
session measures that window 10 points or more under its share, the mod marks
the trigger expired. The margin keeps a session whose reading lags from
clearing a real crossing. Each session that sees the expired trigger can then
be told again if the window crosses later.

## Handoff lines

When the told turn ends, the session shows `handoff: cd <dir> && claude
--resume <id>  # <its handoff line>` and keeps it in `sessions/<id>.json`,
with the keys of the crossings it was told about (`context-tokens` for the
context limit). `/handoff-lines` prints every session's line. The mod does not
end a session and cannot make the model obey the prompt.
