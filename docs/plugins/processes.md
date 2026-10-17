# Plugin processes

This file holds detail for docs/plugins.md. It says how Board stops a plugin
and the processes it starts.

On Unix, the plugin leads a new process group. Past a limit, Board sends
SIGTERM to that group, waits up to a second for the plugin to exit, then sends
SIGKILL to the group. Every process that stays in the group stops with it,
including a `board` the plugin runs; `setsid` and `timeout` leave the group.
Board stops reading, so a process still holding a pipe cannot keep it waiting.
When Board receives SIGINT, SIGTERM or SIGHUP, as on Ctrl-C, it forwards the
signal to the group and then stops. Once Board has started a plugin, it does
not respect an inherited ignored SIGINT or SIGHUP, so `nohup board call`
stops Board and its plugin on hangup.

A nested Board leaves its plugin in the outermost plugin's group, which the
outermost Board signals. Past its own limit, a nested Board signals only its
plugin, so a `run` command must `exec` its final program or forward SIGTERM,
as `uv run` does. Off Unix, Board kills only the plugin.
