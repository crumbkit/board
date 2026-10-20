# Errors

This file holds detail for docs/architecture.md. It lists the codes, the shape
of a report and an error, and the values an error names.

## Codes

The codes, which change as STANDARDS.md 3 and 4 say:

| Area | Reasons |
| --- | --- |
| `json` | `parse`, `merge_conflict`, `duplicate_key`, `too_large`, `not_utf8`, `lossy_number` |
| `record` | `bad_type`, `unknown_type`, `invalid`, `not_a_record` |
| `config` | `not_found`, `invalid`, `ambiguous` |
| `schema` | `invalid` |
| `ref` | `malformed`, `dangling`, `wrong_type` |
| `path` | `malformed`, `shadowed`, `case_collision`, `not_found` |
| `repo` | `unreadable`, `submodule` |
| `mount` | `invalid`, `repo_missing`, `commit_missing`, `remote_unsupported`, `follow_missing`, `not_ancestor`, `stale` |
| `resolve` | `cycle`, `too_many_hops` |
| `index` | `failed`: the disposable index could not be used, even in memory |
| `query` | `invalid`: the SQL does not parse, would write, or is more than one statement |
| `query` | `timeout`: the statement ran longer than 1 s |
| `refs` | `target_changed`: with `--field`, a target's file changed after the check, so its values were not read |
| `put` | `stale`, `locked`, `through_mount`, `symlink`, `invalid`, `not_a_file` (a `--file` path names a directory or ends `.json`), `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `sync` | `no_remote`, `failed` (git refused the fetch or push), `conflict`, `edited`, `gave_up`; see docs/architecture/writes.md |
| `plugin` | see docs/plugins.md |
| `output` | `over_budget`: the output reached 64 MiB, or `BOARD_OUTPUT_BUDGET` bytes when that variable is smaller. `get` and `refs` leave the paths in `left_out` unanswered and exit 1. `query` stops before the next row, carries this code beside its rows and exits 0; it refuses a first row or one value longer than the budget and exits 1. Any other result that is not a listing that can be cut to fit is refused and exits 1 |
| `cli` | `usage`: the command line was refused, exit 2 |

## Reports and failures

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

## What an error names

A `board/failure/v1` document carries `fields` when the error knows any value at
fault. Each key appears only when Board knows its value. The table lists the
keys of Board's own codes; [docs/plugins.md](../plugins.md) says what a
plugin's own code carries.

| Key | JSON type | Codes that carry it | Meaning |
| --- | --- | --- | --- |
| `path` | string | a code about one path, such as one from resolving it (`path::not_found` or `mount::commit_missing`), reading it with `get`, writing to it with `put`, updating the mount there, or syncing a change to it (`sync::conflict`, `sync::edited`) | the logical path at fault: a dangling link, a mount for a mount's code or for `put::through_mount`, or the file that a refused `put` reaches for `record::not_a_record` |
| `ancestor` | string | `path::not_found` | the deepest ancestor of the missing path that exists. The hint lists it, unless the path ends `.json` and the path without `.json` resolves; then the hint names that path |
| `blob` | string | a code `get` gives for a file whose bytes it read (`json`, `record`, and `schema::invalid` for a schema in the reserved `board` namespace); `put::stale` | the blob the path holds |
| `pointer` | string | `record::invalid`, `ref::malformed`, `plugin::bad_input`; `schema::invalid` from `put`, as `/$type` | a JSON pointer into the record or the call's input |
| `type` | string | `record::unknown_type`, `repo::unreadable` and `schema::invalid` from a `put` whose record's schema is missing, cannot be read or does not compile; `plugin::schema_missing` and `plugin::schema_differs` from `call` | the record's type |
| `schema` | string | the same as `type` | that type's schema file, by its path from the board's root |
| `directory` | string | a code that a directory has of its own, such as `path::case_collision`, refusing a `put` beneath it | that directory, by its path from the board's root, without a leading `/` |
| `plugin` | string | `plugin` codes from `call` that name one plugin; every plugin's own code | the plugin |
| `capability` | string | `plugin::not_installed`, `plugin::ambiguous`, `plugin::bad_input` | the capability called |
| `offered` | array of strings | `plugin::not_installed` when the named plugin lacks the capability | each capability that plugin offers |
| `nearest` | string | `plugin::not_installed` when no plugin offers the capability and one offered name is near | the nearest offered name |
| `plugins` | array of strings | `plugin::ambiguous` | each enabled plugin that offers the capability |
| `stderr` | string | `plugin::failed`, `plugin::timeout`, `plugin::too_large`, `plugin::bad_output`; every plugin's own code; `sync::failed` | the end of the plugin's stderr, or of git's, with bytes that are not UTF-8 replaced |
| `tables` | array of strings | `query::invalid` for a missing table | every table and view, and each attached plugin database |
| `columns` | object of string arrays | `query::invalid` for a missing column | each table or view the statement names, mapped to its columns; `{}` when it names none |
| `found` | string or null | `put::stale` | the blob the path holds, or `null` for nothing |
| `refusals` | array of objects | `put::invalid` and `put::stale` for a batch | each refused line, in order, with its `line` (from 1), `code`, `message` and, when it names any, its own `fields` |

A stale put's hint gives the `--expect` that matches. When `columns` is `{}`,
the hint points at `sqlite_master`. Text output prints, after the `code:` line,
one `line <n>: <code>: <message>` line for each of a batch's `refusals`, then
the plugin's stderr.

The check and put reports name a degraded entry's first failing pointer, why,
and how many more (`pointer`, `reason`, `failures`). `reason` is a predicate
about the entry, such as `holds a reference at "/project": nothing is at
/nowhere`. When a record's schema is
missing, cannot be read or does not compile, a check entry names that schema
file (`schema`), as a refused put does.
A check entry's reason for a file that does not parse is `is invalid:` and the
parser's message. For a schema it is `is invalid:` and why the schema does not
compile, or that it is in the `board` namespace, which is Board's own; for a
plugin manifest it is `is invalid:` and why the manifest does not read. The
index keeps the reason with the blob's code, so a warm check reads no file
again. `check` gives each degraded entry a `hint`, filled in with the path
at fault: the entry's own, the reference that dangles, the link or mount a
pointer fails at, or the schema. When that file is in a mounted board, which
Board cannot write, the hint says to fix it in that board's repository. Text
prints each hint on stderr as `hint: <path>: <hint>`.
