# The rules mod's remembered answers

This file holds detail for [../rules-claude-code.md](../rules-claude-code.md).
It says what the rules mod keeps across sessions, and why.

The engine waits for the start hook before the first request, and a check on
a large board can take several seconds. So the mod keeps the last good answer
for each repository root in its `$.store`, which Claude Code keeps on this
machine across sessions. An entry holds the time of the check, each rule's
path, kind, requirement, usage and close line, and the Board badge's name.
The mod keeps the 16 roots checked most recently. It stores no failed answer,
and it removes a root that no board holds.

- A root with an entry starts with those rules in the block, with the time of
  their check, and waits on no Board command. The check then runs in the
  background, for up to 120 seconds. When its answer tells other rules, the
  mod appends one hidden row that gives them. When it fails, the session
  keeps the remembered rules, the debug log says why, and the session is
  told, with Board's code, that the rules may be out of date: in the block
  if the first message has not gone, otherwise in one hidden row.
- A root with no entry waits up to 5 seconds for the check. If the check has
  not answered by then, the block says it is still running, and its answer
  follows as one hidden row.

The entry is keyed by root, not by the board's commit, because a busy board
commits about once a minute. No answer depends on it: every session's own
check replaces it.
