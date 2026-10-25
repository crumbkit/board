# Reports and failures

This file holds detail for docs/architecture/errors.md: the shape of a report,
of an error, and of its recovery.

`--json` prints a command's report in crumbkit's envelope: `schema`
`board/<command>-report/v2`, the report as `data`, and `failures`, one failure
document for each path refused. A listing adds `total`, `truncated` and
`summary`, which holds what describes the whole listing, such as `query`'s
`columns`. The envelope has no field for notes, so they go to stderr. Text
prints a note before any failure.

Board prints an error in crumbkit's `board/failure/v1` shape: `code`,
`message`, `fields` and `recovery`. A failure inside a report's `failures` also
has `key`, the path asked for, and `index`, its position among the paths asked.
A command that fails as a whole prints the document alone and exits 1, or 2
for `board::cli::usage`.

`recovery` says what to try next. Its `kind` is `run`, with `call`, a command
line that runs as written; `hint`, with `text`, a sentence for a reader, used
when the next step needs a value only the reader has; or `none`, with
`reason`, such as `the plugin gave no hint` for a plugin's own code. A path
refused only for its form, such as one without its leading `/`, gets a `run`
recovery holding the corrected line, with any word quoted that a shell would
split, and an `alternatives` array holding one `{kind: run, call}` for
`board <command> -h`. This holds for an optional path, such as `check`'s, too.
A path whose corrected line still does not parse, and a refused `--field`
pointer, get the recovery `board <command> -h` and no `alternatives`.

Text prints the same failure as `error: <message>`, then a `key:` line when it
has one, a `help:` line holding the call or the hint text, a `help:` line for
each alternative, and a `code:` line.
A `get` or `refs` run past its byte budget gives one failure, without a `key`,
whose `fields` hold `left_out` (each path not answered), `left_out_from` and
`budget`. Its recovery is `run` with `all` `left_out` and a `call` that
repeats the run's own words, such as `board -C sub get <PATH>... --json`: run
it with every path in `left_out`. Text prints that command with the paths
filled in, on stderr.
