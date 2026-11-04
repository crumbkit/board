# Plugins still in Python

This file holds detail for docs/plugins.md. It describes the plugins here
that are still Python programs: `tasks`, `changes` and `review`.

Board validates a call's input and output, and `call` checks that the board
holds the schemas the plugin ships. A plugin reading records with `board query`
selects rows whose `status` is `ok`, or whose `code` is `ref::dangling` or
`ref::wrong_type`, and need not validate them again. It still checks what no
schema states, and records in mounted boards, which their own schemas
validate.

These plugins use Python 3.12 or newer with uv, pydantic v2, ruff, pyright
in strict mode and pytest (STANDARDS.md P1 to P5). Each talks the
protocol through the shared library in `python/board-plugin/`, which also
runs `board`: Board's error becomes `<plugin>::board::refused`, carrying
Board's code. Its `repos`, `day`, `writing` and `slug` modules are what the
`board-repos` crate ports for built-in plugins, and its `migrate` module
builds a migrate capability ([lifecycle/updating.md](lifecycle/updating.md)).

Each lives in `plugins/<name>/` with its lockfile and a committed
`plugin.json` that a test compares with the one it generates.
Its pytest suite drives the real `board` binary against fixture boards.
CI's `python` job runs every Python project's gates.
