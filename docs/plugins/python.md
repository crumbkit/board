# The Python library

This file holds detail for docs/plugins.md. Every plugin in this repository
is now a built-in crate, and none is a Python program. The shared library in
`python/board-plugin/` remains for process plugins kept outside it.

The library talks the plugin protocol and runs `board`: Board's error
becomes `<plugin>::board::refused`, carrying Board's code. Its `repos`,
`day`, `writing` and `slug` modules are what the `board-repos` crate ports
for built-in plugins, and its `migrate` module builds a migrate capability
([lifecycle/updating.md](lifecycle/updating.md)). It ships the signing
vectors, so a plugin that pins it reads the bytes Board tests against.

It uses Python 3.12 or newer with uv, ruff, pyright in strict mode and
pytest (STANDARDS.md P1 to P5). Its pytest suite drives the real `board`
binary, and `test_limits.py` checks Board's time limit on a process plugin.
CI's `python` job runs its gates.
