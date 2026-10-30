# Errors

This file holds detail for docs/architecture.md. It lists the codes and the
values an error names.

## Codes

The codes, which change as STANDARDS.md 3 and 4 say:

| Area | Reasons |
| --- | --- |
| `json` | `parse`, `merge_conflict`, `duplicate_key`, `too_large`, `not_utf8`, `lossy_number` |
| `frontmatter` | `parse`, `unsupported`; see docs/architecture/records/frontmatter.md |
| `record` | `bad_type`, `unknown_type`, `invalid`, `not_a_record` |
| `config` | `not_found`, `invalid`, `ambiguous` |
| `schema` | `invalid`, `differs` |
| `ref` | `malformed`, `dangling`, `wrong_type` |
| `path` | `malformed`, `shadowed`, `case_collision`, `not_found` |
| `repo` | `unreadable`, `submodule` |
| `symlink` | `outside`, `loop`, `dangling`; see docs/architecture/paths/symlinks.md |
| `mount` | `invalid`, `repo_missing`, `commit_missing`, `remote_unsupported`, `follow_missing`, `not_ancestor`, `stale` |
| `resolve` | `cycle`, `too_many_hops` |
| `index` | `failed`: the disposable index could not be used, even in memory |
| `query` | `invalid`: the SQL does not parse, would write, or is more than one statement |
| `query` | `timeout`: the statement ran longer than 1 s |
| `refs` | `target_changed`: with `--field`, a target's file changed after the check, so its values were not read |
| `put` | `stale`, `locked`, `through_mount`, `symlink`, `invalid`, `not_a_file` (a `--file` path names a directory or ends `.json`), `nul` (a file's text holds NUL), `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `sync` | `no_remote`, `failed` (git refused the fetch or push), `conflict`, `edited`, `gave_up`, `timeout` (git ran past 60 s); see docs/architecture/writes.md |
| `attachment` | `missing`: `get --fetch` found nothing at the record's location; `mismatch`: the bytes there hash to another digest |
| `plugin` | see docs/plugins.md |
| `settings` | `asked`, `refused`, `from_plugin`, `no_personal`, `not_found`, `unsigned`; see docs/architecture/writes/effects.md |
| `ui`, `serve` | see docs/architecture/ui/views.md and serve.md |
| `output` | `over_budget`: the output reached 64 MiB, or `BOARD_OUTPUT_BUDGET` bytes when that variable is smaller. `get` and `refs` leave the paths in `left_out` unanswered and exit 1. `query` stops before the next row, carries this code beside its rows and exits 0; it refuses a first row or one value longer than the budget and exits 1. Any other result that is not a listing that can be cut to fit is refused and exits 1 |
| `cli` | `usage`: the command line was refused, exit 2 |

## Reports and failures

[errors/reports.md](errors/reports.md) gives the shape of a report, a
`board/failure/v1` error and its `recovery`.

## What an error names

A `board/failure/v1` document carries `fields` when the error knows any value at
fault. Each key appears only when Board knows its value. The table lists the
keys of Board's own codes; [docs/plugins.md](../plugins.md) says what a
plugin's own code carries.

| Key | JSON type | Codes that carry it | Meaning |
| --- | --- | --- | --- |
| `path` | string | a code about one path, such as one from resolving it (`path::not_found` or `mount::commit_missing`), reading it with `get`, writing to it with `put`, updating the mount there, or syncing a change to it (`sync::conflict`, `sync::edited`, `sync::kept`, `put::symlink`) | the logical path at fault: a dangling link, a mount for a mount's code or for `put::through_mount`, or the file that a refused `put` reaches for `record::not_a_record` |
| `ancestor` | string | `path::not_found` | the deepest ancestor of the missing path that exists. The hint lists it, unless the path ends `.json` and the path without `.json` resolves; then the hint names that path |
| `blob` | string | a code `get` gives for a file whose bytes it read (`json`, `record`, and `schema::invalid` for a schema in the reserved `board` namespace); `put::stale` | the blob the path holds |
| `pointer` | string | `record::invalid`, `ref::malformed`, `plugin::bad_input`; `schema::invalid` from `put`, as `/$type` | a JSON pointer into the record or the call's input |
| `type` | string | `record::unknown_type`, `repo::unreadable` and `schema::invalid` from a `put` whose record's schema is missing, cannot be read or does not compile; `plugin::schema_missing` and `plugin::schema_differs` from `call`; each `type add` or batch schema line refusal but a failed write | the record's type |
| `schema` | string | as `type`, but `type add` and batch schema lines set it only on `schema::differs` and a `json` code for the board's copy | that type's schema file, by its path from the board's root |
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
| `target` | string | a `symlink` code | the symlink's text, cut to 100 characters and its length when longer, with backslashes and the characters a logical path may not hold escaped |
| `effect` | string | `settings::asked`, `settings::refused` | the pending effect's id, which `board answer` takes |
| `offset` | number | `put::nul` | the byte offset of the first NUL in the text, which the hint names |
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
