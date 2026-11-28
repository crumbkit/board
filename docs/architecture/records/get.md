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
library's `Board.get` does that. `rules.check` reads every
file its measured rules reach this way.

## Notes beside a record

A built-in plugin declares, with `Plugin::get_notes`, a `GetNote` for a
record type: a function that gives the one path to read beside such a
record, and one that turns the record and what is there into a line or
nothing. After the files are read, `get` reads every path beside the
records asked for in one more read, in the working tree or at the same
commit for `--at`, and prints each line on stderr in text and in `--json`.
stdout keeps the file's bytes, and `board/get-report/v2` gains no field. A
read beside a record that fails other than by finding nothing gives a line
that names its code, and the file and exit code are kept.

The plans plugin declares one for `plans/question/v1`. Beside an open
question at `/…/questions/<id>` it reads `/…/answers/<id>`, under the same
mount. An answer there that names the question, by the path the reader
asked or by its path in the board that holds both, prints
`<question>: answered by <answer> (<answered>): <choice>, "<words>"`, with
the first 80 characters of the words and `none` for no choice. Anything
else prints `<question>: no answer at <answer path>; run board refs
<question> to find an answer at another path`. Board looks no further,
because an indexed lookup costs about 250 ms and would put `get` past its
25 ms budget. A question's stored `status` stays `open` once it is
answered, so this line is how a reader of `get` learns of the answer.

[history.md](history.md) says how `get --at` reads every path at one commit,
and how `board log` lists the commits that changed a path.
