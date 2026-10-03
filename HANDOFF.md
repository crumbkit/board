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
  coordination"): built M2 to M5 in viitta on 2026-10-03, as small pull
  requests that the vision session reviews and merges.
- **M6 builder** `session_019veyTvU3AkfugjSEh9XC5c`: built M6 on 2026-10-03,
  started fresh from the docs when the first builder's context ran long.
- **M1 builder** `session_017XzJQ42de6xdS5bYk5GE9B`: commissioned on
  2026-10-03 for the export script under `tools/export-old-board/`, as one pull
  request; the owner runs it.
- **Design reviewer**: the seed's designer, `session_01YCLTnYNfyNqGPM5UDViA5C`,
  reviews viitta against the original design at the owner's request. The
  vision session lands the changes; the design reviewer reviews them.
- **Mount-walk builder** `session_01RpFWrPnQzNsnZhrpKMUsou` and **docs builder**
  `session_01Vzqbj3yKt1E4VR8cTLvFyn`: commissioned 2026-10-03 from that review.

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

1. **Web UI and sharing** moved to PLAN.md (M8, Views, Serving) on
   2026-10-03.
2. **M1, the old-corpus export.** The script is commissioned; the owner runs it
   against the installed old `board`, into its own repository.

## Next actions

1. M7 is the owner's: run the M1 export, time it with
   `cargo xtask bench --board <export>`, enable `tasks` and use Board daily.
   Route what that finds to a builder as small fixes; route export problems to
   the M1 builder.
2. If a real board misses a budget, three measured speedups for a warm check
   exist (#33): carry validations from refresh into decide (about 25 ms), one
   join per run instead of point lookups (30 to 40 ms), and pointer facts for
   resolution (most of 52 ms, but it touches rule 3, so it needs a design call).
   The stress test (crumbkit/viitta-stress-test at 372ab30) found warm
   check is about 20 µs per entry, and that check is quadratic in pointer-chain
   length: 3.1 s for 500 chains of 39 hops. Memoising was declined while real
   chains stay short; those numbers are the baseline if that changes.
3. Sharing is sketched as M8 in PLAN.md, with the owner's decisions; turn it
   into a design-gap list for a builder after M7. Moving mount walking into
   `board-tree` lands first.
