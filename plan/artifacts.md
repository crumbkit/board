# Artifacts

Every claude.ai artifact the owner publishes is recorded in the board, with
a copy of each version's page from the time the mod is enabled. An artifact belongs to the account that
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
  "first_seen": "2026-10-31T18:20:00Z"
}
```

- `url`, `account` and `first_seen` are required. `first_seen` is the
  first publish the mod saw, or for an import the listing row's `updatedAt`,
  or the import time when the row has none. `<id>` is the
  last segment of the URL.

`artifacts/version/v1` at `/artifacts/<id>/versions/<version>.json`, with
the page beside it as `<version>.html` or `<version>.md`:

- `version` is the version id the Artifact tool returns, such as
  `1793473476-1c42`. The tool's result types make it optional. A result
  without one is keyed by the first 16 hex digits of the page's `sha256`,
  and the record leaves `version` out.
- `title` and `description` are the artifact's title and description at
  that version, since a republish can change them. `title` comes from the
  result. A result carries no description, so `description` is recorded
  only when the publish input passed one.
- `for` is an optional list of references to the changes, evals or projects
  the version belongs to. The mod sets it from the publishing session's
  `Board:` line or its held change. The artifact belongs to every `for` of
  its versions, so an artifact can gain one with a later version. The first
  artifacts build leaves `for` out, since claims and agent records come
  later; the runs mod pull request of the inflight step adds it.
- `sha256` is the page's hash, `source` the local file published, and
  `session` and `at` say who published it and when.
- A document written through a docs connector has no local page, so its
  version has no copy and says `copy: "none"`.

The newest version record is the artifact's current version. The artifact
record never changes.

## Keeping the records current

An `artifacts` mod for Claude Code writes the records. It runs Board as
`board --personal`, as every call these plans add does (plan/work-records.md), so
the records land in the personal board whatever the session's directory.

- After every successful publish, it writes the artifact record if it is
  missing, and a version record with the page copy. The copy is written by
  `board --personal put --file <source>`, which the mod runs through
  `$.process.run`, because `$.fs.read` refuses files over 4 MiB and a page
  can be 16 MB.
- `account` is `oauthAccount.emailAddress` in `~/.claude.json`, read with
  `jq` through `$.process.run`, since that file can also pass 4 MiB.
  `$.session.identity()` would refuse a mod installed from a marketplace.
  When the field is missing, the mod writes nothing and shows a notice.
- After an Artifact read, it writes a version record when the version is not
  recorded yet, with the page that came back. A refused publish has no
  result shape the mod can rely on, so the mod records nothing from it, and
  the read that follows a refusal records the live version. This catches edits made elsewhere: in the
  web app, by a page that saves itself, or from another conversation.

A plugin cannot call the Artifact tool, so artifacts published outside
Claude Code reach the board through the agent. The plugin's skill has an
`artifacts sync` step: the agent runs the Artifact `list` action with
`limit` 200, saves its result unchanged in `listing.json`, and passes it
to:

    jq -c '{account: "<email>", listing: .}' listing.json | board --personal call artifacts.import

`artifacts.import` writes an artifact record for each URL the board does not
have, without a version and without `for`. The step then reads each new
artifact and lets the mod record its version. The same step backfills an old account: the owner
signs in to it, and the agent runs the step once.

The import has two limits, which the skill states:

- The `list` action returns at most 200 artifacts, most recently updated
  first, and it has no cursor for further pages. The step asks for 200. A
  regular sync catches every artifact updated since the last one, because
  those sort first. A backfill of an account with more than 200 artifacts
  records only the 200 most recent, and `artifacts.import` says so when the
  listing's `truncated` is true, giving its `total`.
- A `read` returns only an artifact's current version. Versions published
  before the mod was enabled are not copied, and the first version record
  of an imported artifact is its version at the import.

## Capabilities

- `artifacts.list [--account <a>] [--for <ref>] [--limit <n>]`: read-only.
  Artifacts with their newest version and its title, newest first.
- `artifacts.import`, as above. It refuses a `listing` that is not the
  Artifact tool's list result, `{artifacts: [{url, title, updatedAt?}],
  truncated?, total?}`, with a twin that is admitted.

`inflight.now`, built in the inflight step, lists each open change's artifacts.

## Build

One pull request: the plugin, its skill and the mod. The tests:

- A publish writes both records, and a second publish of the same artifact
  writes only a version.
- A read that returns an unrecorded version writes it, and a read of a
  recorded one writes nothing.
- `artifacts.import` writes only the missing artifacts.
- `artifacts.import` given a listing with `truncated` true reports it and
  the `total`, and one without does not.
- A result with no version id writes a version keyed by the page's hash.

Done when the 200 most recently updated artifacts of each of the owner's
two accounts are in the board, and a new publish appears in
`artifacts.list` without any step by the owner.
