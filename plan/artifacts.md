# Artifacts

Every claude.ai artifact the owner publishes is recorded in the board, with
a copy of each version's page. An artifact belongs to the account that
published it, and switching accounts hides it. The board keeps the link, what
the artifact was for and its content.

## Records

An `artifacts` plugin owns two types, each written once.

`artifacts/artifact/v1` at `/artifacts/<id>.json` holds metadata only:

```json
{
  "$type": "artifacts/artifact/v1",
  "url": "https://claude.ai/artifact/AbCdEf0123456789xyzQ",
  "account": "owner@example.com",
  "for": "/changes/board-work-records",
  "published": "2026-10-31T18:20:00Z"
}
```

- `url`, `account` and `published` are required. `<id>` is the
  last segment of the URL.
- `for` is optional: a reference to the change, eval or project the
  artifact belongs to.

`artifacts/version/v1` at `/artifacts/<id>/versions/<version>.json`, with
the page beside it as `<version>.html` or `<version>.md`:

- `version` is the version id the Artifact tool returns, such as
  `1793473476-1c42`.
- `title` and `summary` are the artifact's title and description at that
  version, since a republish can change them.
- `sha256` is the page's hash, `source` the local file published, and
  `session` and `at` say who published it and when.
- A document written through a docs connector has no local page, so its
  version has no copy and says `copy: "none"`.

The newest version record is the artifact's current version. The artifact
record never changes.

## Keeping the records current

An `artifacts` mod for Claude Code writes the records:

- After every successful publish, it writes the artifact record if it is
  missing, and a version record with the page copy. `account` is read from
  the signed-in Claude Code account.
- After an Artifact read, or a publish refused because the live version is
  newer, it writes a version record when the version id is not recorded yet,
  with the page that came back. This catches edits made elsewhere: in the
  web app, by a page that saves itself, or from another conversation.

A plugin cannot call the Artifact tool, so artifacts published outside
Claude Code reach the board through the agent. The plugin's skill has an
`artifacts sync` step: the agent runs the Artifact `list` action and passes
the listing to:

    board call artifacts.import --account <email> --file <listing.json>

`artifacts.import` writes an artifact record for each URL the board does not
have, without a version. The step then reads each new artifact and lets the
mod record its version. The same step backfills an old account: the owner
signs in to it, and the agent runs the step once.

## Capabilities

- `artifacts.list [--account <a>] [--for <ref>] [--limit <n>]`: read-only.
  Artifacts with their newest version and its title, newest first.
- `artifacts.import`, as above. It refuses a listing that is not the
  Artifact tool's shape, with a twin that is admitted.

`inflight.now` lists each open change's artifacts (plan/inflight.md).

## Build

One pull request: the plugin, its skill and the mod. The tests:

- A publish writes both records, and a second publish of the same artifact
  writes only a version.
- A read that returns an unrecorded version writes it, and a read of a
  recorded one writes nothing.
- `artifacts.import` writes only the missing artifacts.

Done when the owner's artifacts from both accounts are in the board and a
new publish appears in `artifacts.list` without any step by the owner.
