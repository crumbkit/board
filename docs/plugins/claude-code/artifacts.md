# board-artifacts

This file holds detail for docs/plugins/claude-code.md. It says what the
artifacts plugin's mod, `plugins/artifacts/claude-code/`, records.
plugins/artifacts/README.md describes the records and capabilities.

The mod hooks the Artifact tool. After the tool answers, it writes through
`board --personal`, so the records land in the personal board whatever the
session's directory. It has no options. A call the tool refused, or that a
hook denied, records nothing, because a refusal has no result the mod can
rely on. The read that follows a refusal records the live version.

## The account

`account` is `oauthAccount.emailAddress` in `.claude.json`, in the
directory `CLAUDE_CONFIG_DIR` names or else in `HOME`. The mod reads it with
`jq` through `$.process.run`, because the file can pass the 4 MiB that
`$.fs.read` takes, and `$.session.identity()` refuses a mod installed from a
marketplace. When the field is missing, the mod writes nothing and says so in
the transcript.

## A publish or a read

Each records one version of one artifact, in three puts with
`--expect none`:

1. The artifact record, when it is missing, with `first_seen` the time of
   this publish or read.
2. The page, at `<key>.html` or `<key>.md` beside the version record.
3. The version record.

A put that finds the file there already, `board::put::stale`, writes nothing
and is not an error, so a version recorded earlier writes nothing again. The
version record goes last, so a version record always has its page. `<key>`
is the result's version id, or the first 16 hex digits of the page's
SHA-256 when the result has none.

- **A publish** copies the local file it sent, `file_path`, when it is a
  `.html` or `.md` file. `board put --file` reads it through a shell
  redirect, because a page can be 16 MB, and `shasum -a 256` hashes it. The
  version's `title` is the result's, and `description` is the publish
  input's, when it passed one. A publish with no `file_path`, such as one
  that creates an artifact from an Artifact type, or whose file is a data
  file, records the version with `copy: "none"`. An upload of assets
  (`asset: true`) records nothing.
- **A read** of an artifact's page records the version the result names in
  `artifactRead.ver`, with the page that came back. This catches edits made
  in the web app, by a page that saves itself, or from another conversation.
  The structured result holds no page, so the mod reads the text the model
  reads. A large page is in the file the header names after `saved to`,
  which the mod copies. A smaller one follows the header inline, inside a
  `<cowritten-artifact-html>` tag when another session may have written it,
  and the mod writes that text. A read of a file inside an artifact (`path`
  or `paths`), of an artifact the owner does not own, or whose text holds
  only part of the page, records nothing.

## A listing

After the `list` action with no `scope` (or `mine`), no `type`, no
`type_url` and no `url`, the mod passes the result unchanged to
`board --personal call artifacts.import` with the account. It then adds a
row the person does not see after the tool's result, naming each new
artifact and asking the agent to read it, so that the read records its
version, and passing on a truncated listing's message. The plugin's skill
calls this `artifacts sync`.

## What the transcript shows

Each recorded version adds a dim row, such as
`board: recorded <url> version <key> as /artifacts/<id>/versions/<key>`, and
each import a row with its count. A refused put shows its code and message.
Four refusals go only to the debug log, because they mean the personal board
does not take these records at all: `board::config::not_found`,
`board::record::unknown_type`, `board::plugin::not_enabled` and
`board::plugin::not_installed`.
