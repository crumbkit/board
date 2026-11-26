# Fills and drops

`board put`, `put --new` and `put --batch`, run from the command line, change
a record in memory before its schema check. They fill a field the record's
type lets Board fill when the record leaves it out, and drop a field the
schema does not name. `--strict` turns both off, so the put refuses what the
schema refuses, as it did before. A built-in plugin's put through its host
opens the board again without them, so a plugin's write is checked as given.

## Fills

A built-in plugin declares its fills with `Plugin::fills`, by type and
top-level field. A fill never replaces a field the record holds.

| Type | Field | Filled with |
| --- | --- | --- |
| `plans/question/v1` | `by` | `{"session": <id>}` |
| `plans/question/v1` | `asked` | the put's time, in UTC to the second |
| `plans/question/v1` | `status` | `open` |
| `plans/decision/v1` | `by` | `{"session": <id>}` |
| `plans/decision/v1` | `decided` | the put's time |
| `review/report/v1` | `patch_ids` | each commit of `range` that has a diff, oldest first, as `review.run` records them |

The session is `BOARD_SESSION`, or else `CLAUDE_CODE_HOST_SESSION_ID`. Each
is used only when it is `local_` then lowercase hex and `-`, so an invalid
`BOARD_SESSION` does not hide a valid host session. With neither, `by` stays
absent. A subagent's put names its host's session.

The patch ids are read with git in the directory the put started in. When
that directory lacks a commit of `range`, the field stays absent, and the
refusal names the commit and says to run the put from a checkout that holds
it, or to give `patch_ids`.

A fill that cannot run leaves its field absent. When the check still admits
the record, the put prints `warning: <path>: could not fill <pointer>: <why>`;
when the check refuses it, the refusal ends with the same reason.

## Drops

Where the schema closes an object with `additionalProperties: false`, a field
it does not name is removed, at any depth. A field that only a branch of
`anyOf` or `oneOf` refuses is kept, so the check refuses it. A field one edit
from a field the object lacks, such as `alternative` in a decision with no
`alternatives`, is kept too, and the refusal says to rename it. Each drop prints
one warning on stderr, with the field's pointer and a field the type names
one edit away, such as
`warning: /projects/a/decisions/d: dropped /alternative, which its type does not name; the type has /alternatives`.
A record still refused after a drop is refused with its code, and the
message names the fields dropped.

`--json` lists each field in `filled` and `dropped` of
`board/put-report/v2`, with the record's path and, in a batch, its line.

## Lifecycle

| State | Entered when | Leaves to |
| --- | --- | --- |
| read | the record is parsed from stdin or a batch line | filled |
| filled | each fill of its type ran on a field the record lacks | dropped |
| dropped | each field the schema does not name was removed | checked |
| checked | the schema and reference check ran | written, or refused |
| written | the put wrote and committed, as before | done |

Nothing on disk changes before `written`. A batch takes every line through
`checked` before it writes any, so it still writes all or nothing. A kill
during `filled` or `dropped` leaves the board as it was. A fill that fails,
such as a git failure or timeout in the `patch_ids` fill, leaves its field
absent and never refuses by itself; the check decides, and the put's warning
or refusal says why the fill did not run.
