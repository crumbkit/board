# Running programs and caching values

This file holds detail for docs/plugins/built-in.md. It says how a built-in
plugin's `Host` starts a program, places a directory in the board and
keeps a small value between calls.

- `run(process)` starts a program, such as git, gh or an agent, through
  board-plugin's process module, with `environment` and nothing else, so
  the program is identified as the built-in plugin through `BOARD_PLUGIN`
  and runs one call deeper through `BOARD_CALL_DEPTH`. It runs in the
  directory given, or in a new empty temporary directory, and reads the
  input given on stdin, with the variables in its `added` set over those
  of the same names. It leads its own process group. Every process
  still in that group is killed when the program exits, even while one of
  them holds its output open, past its deadline, past 16 MiB of stdout,
  or when Board gets SIGINT, SIGTERM or SIGHUP. On such a signal the group
  gets the signal and then SIGCONT, its leader up to a second to exit, and
  then SIGKILL, as a plugin's group does (docs/plugins/processes.md). A process that leaves the group with `setsid` is not killed; if it
  holds the output open, Board drains it for up to a second after the
  program exits, even past the deadline. The temporary directory is left behind if a signal ends
  Board. It returns the exit code, stdout and the
  end of stderr, or why it did not start or was stopped.
- `run_hiding(process, hidden)` is `run` without the variables `hidden`
  names, such as model credentials that git and gh must not see; the
  other variables are kept. `run` is `run_hiding` hiding nothing, so a
  `Host` that starts processes its own way, such as a test's recording
  host, overrides `run_hiding` alone and serves both.
- `places(directory)` gives the logical path of `directory` at each mount
  of the board whose repository holds it, as `board refs <directory>`
  names it. It refuses with `board::path::malformed` when one of those
  paths would be too long to be a logical path, so a caller never takes a
  dropped place for no place. A host with no board, such as a test's,
  finds none.
- `cached(place, digest)` and `cache(place, digest, value)` read and
  write a small value under `place`, plain names under the cache home
  such as `board-rules/judged`, in a file named by the hex of
  `digest`, a content hash the plugin derives from what the value
  answers. board-index owns the files. Deleting them loses only the time
  to compute the values again.
