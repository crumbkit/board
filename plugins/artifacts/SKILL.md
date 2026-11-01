---
name: artifacts
description: 'The claude.ai artifacts the owner reviews, each with the task or change it belongs to, what it shows, whether the owner agreed it, and a copy of every version''s page, recorded by the board-artifacts Claude Code mod. Use it to find an artifact, to file one, or to sync the artifacts published outside Claude Code. Start with `board --personal call artifacts.list --subject /changes/<name>`, which lists that work''s artifacts with every version, newest first. SKILL.md and README.md in the plugin''s directory say more.'
---

# artifacts

Find the owner's claude.ai artifacts in the personal board, file each one
against its work, record the owner's agreement, and bring in the artifacts
published outside Claude Code.

The board-artifacts mod records every artifact this session publishes or
reads. You never write the version records yourself.

## Find an artifact

    board --personal call artifacts.list --subject /changes/<name>

It lists each artifact's link, subject, `shows`, `state` and every version
with its title, newest first. `--state shown` keeps one state, `--area
dashboard/flows` one area, `--account <email>` one account, and `--unfiled
true` the artifacts with no subject or no `shows`. `--limit <n>` returns up
to 200 (50 by default). A version's `path` names its record; the page is
beside it as `<path>.html` or `<path>.md`, which `board --personal get`
reads.

## File an artifact

The mod fills `subject` from the `board-brief: /changes/<name>` line that
starts your first message, or else from the task you claimed with
`tasks.claim`. When it says the artifact has no subject or no `shows`, set
them in the same turn:

    board --personal call artifacts.set --path /artifacts/<id> --subject /changes/<name> --shows 'One sentence saying what the page is for.'

`--area <tag>` adds an area tag such as `dashboard/flows`. Run `set` again
to add a second subject or area. After the owner has seen the page, set
`--state shown`.

When the mod says the new artifact's subject already has a shown or agreed
artifact, ask yourself whether the new one replaces it. If it does, run
`artifacts.set --path <new> --supersedes <old>`, which marks the old one
`superseded`.

## Record the owner's agreement

Only the owner's decision moves an artifact to `agreed`. Record the owner's
answer and a `plans/decision/v1` whose `refs` name that answer, then run:

    board --personal call artifacts.agree --path /artifacts/<id> --decision <decision>

`artifacts::agree::no_decision` means the decision is missing, is a hook's
stub, or names none of the owner's answers.

## artifacts sync

Run this step when the owner asks to sync artifacts, or to backfill an
account the owner has just signed in to.

1. Run the Artifact tool's `list` action with `limit` 200 and no `scope`
   or `type`. The mod passes the listing to `artifacts.import`, which
   writes an artifact record for each link the board does not have. The
   mod then tells you which artifacts are new.
2. Read each new artifact with the Artifact tool's `read` action and its
   `url`. The mod records the version that comes back. You need not read
   the page yourself.

Tell the owner how many artifacts were new, and pass on what the mod says
about a truncated listing.

## Limits

- The `list` action returns at most 200 artifacts, most recently updated
  first, and has no way to ask for more. A regular sync still catches every
  artifact updated since the last one, because those sort first. A backfill
  of an account with more than 200 artifacts records only the 200 most
  recent, and the mod says how many the account has.
- A `read` returns only an artifact's current version. Versions published
  before the mod was enabled are not copied, and the first version record
  of an imported artifact is its version at the import.
