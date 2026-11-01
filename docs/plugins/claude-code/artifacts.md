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

Each records one version of one artifact, in three steps:

1. `artifacts.record`, which writes the artifact record when it is missing,
   with `first_seen` the time of this publish or read.
2. A put of the page, at `<key>.html` or `<key>.md` beside the version
   record, with `--expect none`.
3. A put of the version record, with `--expect none`.

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
  (`asset: true`) records nothing. A publish whose result says
  `owned: false`, to another person's artifact the owner may edit, records
  nothing, so only the owner's own artifacts are recorded.
- **A read** of an artifact's page records the version the result names in
  `artifactRead.ver`, with the page that came back. This catches edits made
  in the web app, by a page that saves itself, or from another conversation.
  The structured result holds no page, so the mod reads the text the model
  reads. The header is the first line that starts with `[Artifact `, since
  a notice, such as one saying the read is of a newer version, may come
  first. A large page is in the file the header names after `saved to`,
  which the mod copies. A smaller one follows the header inline, after any
  bracketed notes such as the version's file count and what is stored for
  it, which the mod leaves out. The page is inside a
  `<cowritten-artifact-html>` tag when another session may have written it,
  and the mod writes that text. A version recorded from a read holds the
  page as claude.ai serves it, wrapped in the skeleton a publish adds, so its
  bytes and hash differ from the source file a publish of the same content
  sent. The Markdown form of the header, `raw Markdown follows`, is assumed;
  no real transcript has shown it yet. A read of a file inside an artifact (`path`
  or `paths`), of an artifact the owner does not own, or whose text holds
  only part of the page, records nothing.

## The subject

A publish passes `artifacts.record` the work the conversation belongs to. It
is the change that a `board-brief: /changes/<name>` line names, when that
line starts the conversation's first user message, which
`$.session.messages` gives for the main session and for a subagent alike.
Without one, it is the task the conversation last claimed: after a Bash call
that runs `tasks.claim` succeeds, the mod keeps the absolute `--path` it
named for that conversation. A read passes no subject.

After a publish, the mod adds a row the person does not see after the
tool's result when the record lacks a subject or `shows`, giving the
`artifacts.set` command that fills each. When `artifacts.record` names
`may_replace` artifacts, the row names them and asks the agent to run
`artifacts.set --supersedes` if the new artifact replaces one.

## A listing

After the `list` action with no `scope` (or `mine`), no `type`, no
`type_url` and no `url`, the mod passes the result unchanged to
`board --personal call artifacts.import` with the account. It then adds a
row the person does not see after the tool's result, naming each new
artifact and asking the agent to read it, so that the read records its
version, and passing on a truncated listing's message. It adds that row
only when the call's `limit` was 200, which is the plugin's skill's
`artifacts sync` step; any other listing is imported without asking the
agent anything. It calls a listing truncated only when it holds 200 rows,
since a shorter one that says it is truncated was cut by its own limit.

## What the transcript shows

Each recorded version adds a dim row, such as
`board: recorded <url> version <key> as /artifacts/<id>/versions/<key>`, and
each import a row with its count. A refused put or call shows its code and
message. Four refusals go only to the debug log, because they mean the
personal board does not take these records at all: `board::config::not_found`,
`board::record::unknown_type`, `board::plugin::not_enabled` and
`board::plugin::not_installed`.
