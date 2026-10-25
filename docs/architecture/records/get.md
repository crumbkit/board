# board get

This file holds detail for docs/architecture.md, Commands.

`board get` serves an agent that reads several records: it names them all in
one run instead of starting one run each. With one path, text output is the
file's bytes. With several, each file follows a `==> <path> <==` line, as
`head` prints them, and a blank line separates files. That text cannot always
be split back into each file's exact bytes, for example when a file lacks a
final newline; `--json` is the exact form. It is always one
`board/get-report/v2`, for one path too. `data` holds, in the order
asked, each file's Location, whose source is `worktree` or a commit id, its
blob, its bytes, and either its `record` or the `code` saying why it is not
one. On an answer, `code: board::record::not_a_record` means a readable file
that holds no record, such as markdown. The bytes are a JSON string under
`text` when they are UTF-8, and `base64` otherwise. A path that names a
directory or nothing is refused in `failures`, with the `index` of its
path; the other paths are still answered, and the run
exits 1. One run takes at most 10,000 paths; more is `board::cli::usage`. Like
every command, `get` collects its output in memory and writes it at exit. It
reads each file only once the answers before it are rendered, and only while
the output, counted in the format being printed, is under 64 MiB; the first
path is always read. One `board::output::over_budget` failure names the later
paths in `left_out`, to ask for in another run. So a run holds at most
64 MiB, one more answer, and the short errors of the paths it left. The plugin
library's `Board.get` does that. `controls.check` reads every
file its measured controls reach this way.
