# Running programs and caching values

This file holds detail for docs/plugins/built-in.md. It says how a built-in
plugin's `Host` starts a program and keeps a small value between calls.

- `run(process)` starts a program, such as git, gh or an agent, through
  board-plugin's process module, with `environment` and nothing else, so
  the program is identified as the built-in plugin through `BOARD_PLUGIN`
  and runs one call deeper through `BOARD_CALL_DEPTH`. It runs in the
  directory given, or in a new empty temporary directory, and reads the
  input given on stdin. It leads its own process group. Every process
  still in that group is killed when the program exits, even while one of
  them holds its output open, past its deadline, past 16 MiB of stdout,
  or when Board gets SIGINT, SIGTERM or SIGHUP. On such a signal the group
  gets the signal and then SIGKILL at once, so its own handlers rarely
  run. A process that leaves the group with `setsid` is not killed; if it
  holds the output open, Board drains it for up to a second after the
  program exits. The temporary directory is left behind if a signal ends
  Board. It returns the exit code, stdout and the
  end of stderr, or why it did not start or was stopped.
- `cached(place, digest)` and `cache(place, digest, value)` read and
  write a small value under `place`, plain names under the cache home
  such as `board-controls/judged`, in a file named by the hex of
  `digest`, a content hash the plugin derives from what the value
  answers. board-index owns the files. Deleting them loses only the time
  to compute the values again.
