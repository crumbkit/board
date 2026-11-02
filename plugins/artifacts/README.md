# artifacts

A plugin built into Board that records the owner's claude.ai artifacts: the
link, the account that published it, the change or task it belongs to, what
it shows, where the owner's review stands, and a copy of each version's page
from the time the board-artifacts mod is enabled. An artifact belongs to the
account that published it, and switching accounts in claude.ai hides it, so
the board keeps what the account would hide. The records live in the
personal board. Each schema's descriptions hold the meaning of every field.

## The artifact record

`/artifacts/<id>.json`, where `<id>` is the last segment of the link.

```json
{
  "$type": "artifacts/artifact/v1",
  "url": "https://claude.ai/artifact/AbCdEf0123456789xyzQ",
  "account": "owner@example.com",
  "first_seen": "2026-10-31T18:20:00Z",
  "subject": ["/changes/flows-editor"],
  "shows": "The step list that replaces the flow graph.",
  "state": "agreed",
  "agreed_by": "/projects/flows/decisions/20261101T0900Z-step-list",
  "area": ["dashboard/flows"]
}
```

- `url`, `account`, `first_seen` and `state` are required.
- `first_seen` is the first publish or read the mod saw, or for an import
  the listing row's `updatedAt`, or the import time when the row has none.
- `subject` references the change or task the artifact belongs to: a
  `changes/change/v1` or a `tasks/task/v2`.
- `state` is `draft`, `shown`, `agreed` or `superseded`. A new artifact is
  `draft`. The schema requires `agreed_by`, a `plans/decision/v1`, on an
  agreed one. `superseded_by` names the artifact that replaced a superseded
  one.
- `area` holds tags of lowercase parts joined by `/`, such as
  `dashboard/flows`.

## The version record

`/artifacts/<id>/versions/<key>.json`, with the page beside it as
`<key>.html` or `<key>.md`. Each is written once. The newest version record,
by `at`, is the artifact's current version.

```json
{
  "$type": "artifacts/version/v1",
  "version": "1793473476-1c42",
  "title": "Release plan",
  "description": "The plan for the next release.",
  "sha256": "9f2c…",
  "source": "/tmp/scratch/plan.html",
  "session": "0b8a7c6e-1f2d-4c3b-9a8e-7d6c5b4a3f21",
  "at": "2026-10-31T18:20:00Z"
}
```

- `session` and `at` are required: the Claude Code session that published
  or read the version, and when.
- `<key>` is `version`, the version id the Artifact tool returned, or, when
  it returned none, the first 16 hex digits of `sha256`, and the record
  leaves `version` out.
- `description` is recorded only when the publish passed one. `source` is
  the local file a publish sent; a version recorded from a read has none.
- A version with no local page, such as one written through an Artifact
  type, says `copy: "none"` and has no `sha256`. The schema refuses a
  version that has neither, or both.

## Capabilities

`board --personal call artifacts.list` returns the artifacts, newest first
by their newest version, or by `first_seen` for one with none, each with its
fields and every version's `path`, `version`, `title` and `at`, newest
first. `--account`, `--state` and `--area` keep the artifacts with that
value. `--subject <path>` keeps those whose `subject` reaches the path, which
`board refs` decides, so a link to a change finds its artifacts too.
`--unfiled true` keeps those with no subject or no `shows`. `--limit` is 1 to
200 (50 by default). `total` counts every artifact that matched, `truncated`
says whether the limit left some out, and `unreadable` counts the records
Board marks degraded, which are left out. A version belongs to the artifact
whose path is its own up to `/versions/`, so a mounted board's artifacts list
the same way.

`artifacts.import` takes `{account, listing}` on stdin, where `listing` is
the Artifact tool's `list` result unchanged: `{artifacts: [{url, title,
updatedAt?}], truncated?, total?}` and the tool's other fields. It writes a
`draft` artifact record for each link the board does not have, in one
commit, without a version. It returns the paths `written`, how many were
`known`, and `truncated`: null for a whole listing, or how many were listed,
the account's `total` and a message saying the rest were not imported. Board
refuses with `board::plugin::bad_input` a listing that is not that shape,
holds more than 200 rows, has a link that is not an artifact's, or has a
`scope` other than `mine`.

`artifacts.record` takes `{url, account, subject?}` on stdin; the mod calls
it after each publish and read. It writes the artifact record in `draft` when
it is missing, and gives a held record with no subject the one passed. It
returns the record's `path`, whether it was `created`, its `subject`,
`shows` and `state`, and `may_replace`: for a new artifact, the other `shown`
or `agreed` artifacts whose subject reaches one of its own.

`artifacts.set --path <artifact>` adds `--subject` or `--area` when the
record lacks it, replaces `--shows`, and sets `--state` to `draft` or
`shown`. `--supersedes <artifact>` marks that artifact `superseded`, with
`superseded_by` naming this one, in the same commit. It refuses a call with
nothing to change with `artifacts::set::nothing`, a path that holds no
artifact with `artifacts::artifact::not_found`, and a `--state` on an agreed
or superseded artifact with `artifacts::state::settled`.

`artifacts.agree --path <artifact> --decision <decision>` sets `state` to
`agreed` and `agreed_by` to the decision once the owner approves it. The
decision must be a `plans/decision/v1`, or the agree is refused with
`artifacts::agree::no_decision`; a superseded artifact is refused with
`artifacts::state::settled`.

The owner approves each agree with Touch ID through Board's effects. The
plugin asks Board to decide `artifacts.agree` at the artifact's path, showing
the artifact record as it would be written. With no answer yet, Board writes
a pending effect in the personal board and the agree refuses with
`artifacts::agree::asked`, naming the effect; the owner approves with
`board answer <effect> approve`. The approval covers that exact record, so it
admits only the same artifact agreed with the same decision, and a record an
agent writes cannot stand in for it. When the owner refuses, the agree
refuses with `artifacts::agree::refused`.

## Keeping the records current

The board-artifacts mod in `claude-code/` writes the records as a session
works. docs/plugins/claude-code/artifacts.md says what it records and when.
SKILL.md says how an agent files an artifact, and has the `artifacts sync`
step that brings in artifacts published outside Claude Code.
