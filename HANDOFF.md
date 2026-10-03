# Handoff: vision and review session

State of the session that designed this seed (claude.ai session
`session_01YCLTnYNfyNqGPM5UDViA5C`, "Board architecture rethink"), written
2026-10-03 so that a fresh session can take over. Delete this file once its
open threads are settled or moved into PLAN.md.

## Roles

- **Owner** decides: scope, budgets, repositories, merges.
- **Vision and review session** (`session_01D6hYfXRzt3ridFR4gQMEUn`): holds the design, keeps it
  lean, reviews every builder PR against STANDARDS.md and docs/, and changes
  the docs when the design is wrong. It does not implement milestones.
- **Builder session** `session_018fcBgd866UNNdTiVtbJpFc` ("Session task
  coordination"): built M2 and M3 in viitta on 2026-10-03, as small pull
  requests that the vision session reviews and merges.
- **M1 builder** `session_017XzJQ42de6xdS5bYk5GE9B`: commissioned on
  2026-10-03 for the export script under `tools/export-old-board/`, as one pull
  request; the owner runs it.

## Where things are

- The seed was designed as `next/` on crumbkit/board and merged here as
  PR #1 on 2026-10-03, with CI green on Linux and macOS (lint, tests,
  cargo-deny and cargo-audit). The board branch that held it is to be
  deleted by the owner; the git proxy refused the deletion.
- The owner's answers to the seed's open decisions are in PLAN.md (Settled
  decisions).

## How the design was reached

The conversation that produced PLAN.md is not stored anywhere else. Its
conclusions:

- The old Board rebuilt git inside git and put governance in the kernel. Start
  again; take ideas, not files.
- A board is a git tree. Commits are ordinary commits; a branch is a board; a
  merge joins boards. Reftables solve ref storage, a different problem.
- JSON records with `$type`; Markdown only as files a record points at, because
  structure must be enforceable. Agents mostly read and write through `board`,
  which validates and canonicalises.
- Ingest never refuses: a bad file is a path plus a code.
- Everything derived is keyed by blob hash, so "true at this commit" is a join
  and every branch shares the index.
- Links and mounts are one pointer document at the point in the tree, followed
  only by Board. Git symlinks are unsuitable because the link is logical. Mounts
  pin a commit so the whole stays one Merkle tree.
- No query language: agents write SQL or code.
- Plugins are separate processes with a JSON contract, discovery by capability,
  and one SQLite per version whose tables Board never names.
- Resolution complexity lives in one crate, enforced by cargo-deny `wrappers`,
  and is tested from day one with scenarios and property tests.

## Owner preferences

Lean, humble and reluctant growth; the owner says "I keep overengineering", so
push back on scope, including the owner's. The owner writes Ruby, Rust and
sometimes Elixir. They liked domain-named error codes. Nothing depends on the
old Board. macOS first, with other platforms soon. Explain what a test proves
and what it does not.

## Open threads

1. **Web UI and sharing: designed for, not built in v0.** The adapter rule is in
   docs/architecture.md (Crates). The rest waits for a plan:
   - Every record type gets a view with no plugin work: a detail view and a
     form derived from its JSON Schema. A plugin may override it with a
     declarative view in its manifest (data, a small component vocabulary, field
     bindings), so a list of a thousand records spawns no process. A sandboxed
     web component is a later escape hatch for charts and editors. Views from a
     mounted board are data, never code.
   - Editing goes through `put` with `--expect`, which already gives a web form
     safe concurrent saves.
   - The unit of sharing is a repository: a shared project is its own board,
     mounted into each person's board, with permissions from the git host.
     Sharing needs writes through mounts (commit in the mounted repository, then
     move the pin), so that is the first post-v0 feature to plan.
   - Uploads are files in the tree; large binaries need git LFS or a content
     store referenced by hash. Decide when the first real upload exists.
   - The acceptance test, set by the owner: the UI is done when it can express
     widgets's UI. Derive the component vocabulary from that UI when the UI is
     planned; example-org/widgets is not yet in this session's access.
2. **M1, the old-corpus export.** The script is commissioned; the owner runs it
   against the installed old `board`, into its own repository.

## Next actions

1. Commission M4 from the builder, starting with its design-gap list, and
   review its pull requests.
2. #21 (app-name constants) waits for the owner to confirm the request.
3. The owner runs the M1 export; route problems to the M1 builder.
