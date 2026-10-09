# Errors

This file holds detail for docs/architecture.md. It lists the codes and the
values an error names.

## Codes

The v0 set:

| Area | Reasons |
| --- | --- |
| `json` | `parse`, `merge_conflict`, `duplicate_key`, `too_large`, `not_utf8`, `lossy_number` |
| `record` | `bad_type`, `unknown_type`, `invalid`, `not_a_record` |
| `config` | `not_found`, `invalid` |
| `schema` | `invalid` |
| `ref` | `malformed`, `dangling`, `wrong_type` |
| `path` | `malformed`, `shadowed`, `case_collision`, `not_found` |
| `repo` | `unreadable`, `submodule` |
| `mount` | `invalid`, `repo_missing`, `commit_missing`, `remote_unsupported`, `follow_missing` |
| `resolve` | `cycle`, `too_many_hops` |
| `index` | `failed`: the disposable index could not be used, even in memory |
| `query` | `invalid`: the SQL does not parse, would write, or is more than one statement |
| `query` | `timeout`: the statement ran longer than 1 s |
| `put` | `stale`, `locked`, `through_mount`, `symlink`, `invalid`, `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `plugin` | see docs/plugins.md |
| `usage` | `invalid`: the command line was refused, exit 2 |

## What an error names

A `board/error/v1` document carries `fields` when the error knows any value at
fault. Each key appears only when Board knows its value. The table lists the
keys of Board's own codes; [docs/plugins.md](../plugins.md) says what a
plugin's own code carries.

| Key | JSON type | Codes that carry it | Meaning |
| --- | --- | --- | --- |
| `path` | string | a code about one path, such as one from resolving it (`path::not_found` or `mount::commit_missing`), reading it with `get`, writing to it with `put`, or updating the mount there | the logical path at fault: a dangling link, or a mount for a mount's code |
| `blob` | string | a code `get` gives for a file whose bytes it read (`json`, `record`, and `schema::invalid` for a schema in the reserved `board` namespace); `put::stale` | the blob the path holds |
| `pointer` | string | `record::invalid`, `ref::malformed`, `plugin::bad_input`; `schema::invalid` from `put`, as `/$type` | a JSON pointer into the record or the call's input |
| `type` | string | `record::unknown_type`, `repo::unreadable` and `schema::invalid` from a `put` whose record's schema is missing, cannot be read or does not compile | the record's type |
| `schema` | string | the same as `type` | that type's schema file, by its path from the board's root |
| `directory` | string | a code that a directory has of its own, such as `path::case_collision`, refusing a `put` beneath it | that directory, by its path from the board's root, without a leading `/` |
| `plugin` | string | `plugin` codes from `call` that name one plugin; every plugin's own code | the plugin |
| `capability` | string | `plugin::not_installed`, `plugin::ambiguous`, `plugin::bad_input` | the capability called |
| `offered` | array of strings | `plugin::not_installed` when the named plugin lacks the capability | each capability that plugin offers |
| `nearest` | string | `plugin::not_installed` when no plugin offers the capability and one offered name is near | the nearest offered name |
| `plugins` | array of strings | `plugin::ambiguous` | each enabled plugin that offers the capability |
| `stderr` | string | `plugin::failed` when the plugin ran, `plugin::timeout`, `plugin::too_large`, `plugin::bad_output` | the end of the plugin's stderr, with bytes that are not UTF-8 replaced |
| `tables` | array of strings | `query::invalid` for a missing table | every table and view, and each attached plugin database |
| `columns` | object of string arrays | `query::invalid` for a missing column | each table or view the statement names, mapped to its columns; `{}` when it names none |
| `found` | string or null | `put::stale` | the blob the path holds, or `null` for nothing |
| `refusals` | array of objects | `put::invalid` and `put::stale` for a batch | each refused line, in order, with its `line` (from 1), `code`, `message` and, when it names any, its own `fields` |

A stale put's hint gives the `--expect` that matches. When `columns` is `{}`,
the hint points at `sqlite_master`. Text output prints one
`line <n>: <code>: <message>` line for each of a batch's `refusals` between the
error and the hint, and the plugin's stderr after the hint.

The check and put reports name a degraded entry's first failing pointer, why,
and how many more (`pointer`, `reason`, `failures`). When a record's schema is
missing, cannot be read or does not compile, a check entry names that schema
file (`schema`), as a refused put does.
