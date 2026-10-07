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
| `put` | `stale`, `locked`, `through_mount`, `invalid`, `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `plugin` | see docs/plugins.md |
| `usage` | `invalid`: the command line was refused, exit 2 |

## What an error names

An error names the values it knows, and `board/error/v1` gives them in
`fields`: `path`, `blob`, `pointer`, `type`, `schema`, `plugin`, `capability`.
`check` and `put` name a degraded entry's first failing pointer, why, and how
many more (`pointer`, `reason`, `failures`); an unknown type's reason names the
schema file its board lacks (`schema`).
