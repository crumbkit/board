# Plugin processes

This file holds detail for docs/plugins.md. It says how long a call may run,
what environment it gets, and how Board stops a plugin and the processes it
starts.

## Capability limits

A capability in `plugin.json` may set two fields of Board's own:

- `timeout`: whole seconds, from 1 to 3600. It replaces the 30-second limit for
  that capability, longer or shorter. Past it Board stops the plugin with
  `board::plugin::timeout`.
- `env`: names of variables, each made of ASCII letters, digits and `_`, not
  starting with a digit. Board passes each one it inherited, in addition to
  the variables it passes every plugin. A name that Board inherited no value
  for is left out. A name Board sets itself, such as `PATH` or
  `BOARD_CALL_DEPTH`, is refused.

A bad value is `board::plugin::bad_manifest`. `review.run` uses both, to give
an agent half an hour, and to read the agent command and its API key.

## Stopping a plugin

board-plugin's process module runs a plugin through crumbkit-process. On
Unix, the plugin leads a new process group. Past a limit, Board sends SIGTERM
to that group, waits up to a second for the plugin to exit, then sends
SIGKILL to the group. When the plugin exits, Board kills every process still
in its group. Every process that stays in the group stops with it, including
a `board` the plugin runs; a process that should outlive the plugin leaves
the group with `setsid`, and `timeout` leaves it too. A process that left the
group and holds a pipe keeps Board reading for at most a second after the
plugin exits. When Board receives SIGINT, SIGTERM or SIGHUP, as on Ctrl-C,
it sends the signal and then SIGCONT to every group it runs, waits up to a
second for each group's leader to exit, sends SIGKILL to the groups, and then
stops. Once Board has started a plugin, it does not respect an inherited
ignored SIGINT or SIGHUP, so `nohup board call` stops Board and its plugin on
hangup.

A nested Board leaves its plugin in the outermost plugin's group, which the
outermost Board signals. Past its own limit, a nested Board signals only its
plugin, so a `run` command must `exec` its final program or forward SIGTERM,
as `uv run` does. Off Unix, Board kills only the plugin.
