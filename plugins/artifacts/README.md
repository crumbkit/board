# artifacts

A Board plugin that records the owner's claude.ai artifacts: the link, the
account that published it, and a copy of each version's page from the time
the board-artifacts mod is enabled. An artifact belongs to the account that
published it, and switching accounts in claude.ai hides it, so the board
keeps what the account would hide. The records live in the personal board.

## The artifact record

`/artifacts/<id>.json`, where `<id>` is the last segment of the link. It is
written once and never changes.

```json
{
  "$type": "artifacts/artifact/v1",
  "url": "https://claude.ai/artifact/AbCdEf0123456789xyzQ",
  "account": "owner@example.com",
  "first_seen": "2026-10-31T18:20:00Z"
}
```

- All three fields are required. `url` is a `claude.ai/artifact/<id>` or
  `claude.ai/code/artifact/<id>` link.
- `first_seen` is the first publish or read the mod saw, or for an import
  the listing row's `updatedAt`, or the import time when the row has none.

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
  "at": "2026-10-31T18:20:00Z",
  "for": ["/changes/release-plan"]
}
```

- `session` and `at` are required: the Claude Code session that published
  or read the version, and when.
- `version` is the version id the Artifact tool returned. `<key>` is that
  id, or, when the tool returned none, the first 16 hex digits of
  `sha256`, and the record leaves `version` out.
- `title` is the artifact's title at that version, from the tool's result.
  `description` is recorded only when the publish passed one, because the
  result carries none.
- `sha256` is the hash of the page copied beside the record. `source` is the
  local file a publish sent; a version recorded from a read has none.
- A version with no local page, such as one written through an Artifact
  type, says `copy: "none"` and has no `sha256`. The schema refuses a
  version that has neither, or both.
- `for` lists references to the changes, evals or projects the version
  belongs to, so `check` reports one that reaches nothing. The mod sets it
  to the change the session works on. An artifact belongs to every `for` of
  its versions.

## Capabilities

`board --personal call artifacts.list` returns the artifacts, newest first,
each with its newest version's `path`, `version`, `title` and `at`, or a
`current` of null when it has no version yet. An artifact with no version
sorts by `first_seen`. `--account <email>` keeps one account's artifacts,
`--for <ref>` keeps those with a version whose `for` names the reference,
and
`--limit` is 1 to 200 (50 by default). `total` counts every artifact that
matched, `truncated` says whether the limit left some out, and `unreadable`
counts the records Board marks degraded, which are left out. A version
belongs to the artifact whose path is its own up to `/versions/`, so the
artifacts of a mounted board list the same way.

`artifacts.import` takes `{account, listing}` on stdin, where `listing` is
the Artifact tool's `list` result unchanged: `{artifacts: [{url, title,
updatedAt?}], truncated?, total?}` and the tool's other fields. It writes an
artifact record for each link the board does not have, in one commit, without
a version, and leaves a path that holds anything as it is. It returns the
paths it `written`, how many were `known`, and `truncated`: null for a whole
listing, or how many were listed, the account's `total` and a message
saying the rest were not imported. Board refuses with
`board::plugin::bad_input` a listing that is not that shape, holds more than
200 rows, has a link that is not an artifact's, or has a `scope` other than
`mine`, since a listing of shared artifacts is not the account's own.

## Keeping the records current

The board-artifacts mod in `claude-code/` writes the records as a session
works. docs/plugins/claude-code/artifacts.md says what it records and when.
SKILL.md has the `artifacts sync` step that brings in artifacts published
outside Claude Code, and its two limits.
