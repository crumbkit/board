# Writing a plugin in Python

This file holds detail for docs/plugins.md. It says what Board validates for a
plugin, what the plugin still checks, and how the plugins here are built and
tested.

Board validates a call's input and output, and `call` checks that the board
holds the schemas the plugin ships. A plugin reading records with `board query`
selects rows whose `status` is `ok`, or whose `code` is `ref::dangling` or
`ref::wrong_type`, and need not validate them again. It still checks what no
schema states, and records in mounted boards, which their own schemas
validate.

Plugins are written in Python 3.12 or newer with uv, pydantic v2, ruff, pyright
in strict mode and pytest (STANDARDS.md P1 to P5). Each plugin talks the
protocol through the shared library in `python/board-plugin/`, which also
runs `board`: Board's error becomes `<plugin>::board::refused`, carrying
Board's code. Its `repos` module finds the board's notes repos and matches a git
remote to their records, its `day` module dates the person's day by
`BOARD_DAY_START`, and its `migrate` module builds a migrate capability
([lifecycle/updating.md](lifecycle/updating.md)). Its `writing` module writes a
notes repo's file through `board put`, retrying a stale or moved write, then
pushes and moves the mount's pin; its `slug` module turns a note's title into
the file name Reflect gives it.

Each plugin lives in `plugins/<name>/` here, or in its own repository pinning
`python/board-plugin/` to a commit, with its lockfile and a committed
`plugin.json` that a test compares with the one it generates.
Its pytest suite drives the real `board` binary against fixture boards. CI's
`python` job runs the gates of every plugin here.
