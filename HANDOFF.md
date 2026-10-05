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
- **Query builder** `session_01B4LeTuRjaTLC2nLYxzkpTu` (#70) and **capability
  builder** `session_018BtKUJoH2X4FhBkv9CtrqG` (PRs A and B below).
- **Stop, 2026-10-04.** The owner stopped all work. Every builder was told to
  push what it had as a draft PR with a handoff in its body, then stop. Check
  open draft PRs before starting anything.

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

## State on 2026-10-05

At the stop, main was at 3e01665. Landed since the design review (#58 to #70): mount walking
moved into `board-tree` (rule 3); `put` refuses `.git` in any spelling; plugins
get SIGTERM, a 1 s grace, then SIGKILL; schema expansion is bounded across every
in-place applicator; reference search matches patterns as the validator does;
`query` refuses ATTACH, DETACH, PRAGMA and any non-read-only statement and stops
after 1 s. Each has a refusal test with an admitted twin. Since the stop, #77
stopped the plugin time-limit tests failing when the full suite runs on macOS,
and #76 added warm-against-cold tests for `query` and `refs`.

Owner decisions this round: remote mounts change `board/mount/v1` in place
(#59); the docs budget is 5,000 words (PLAN decision 6); #57, the chain memo,
is closed (its branch and numbers are the baseline if long chains appear).

Landed after the stop: #71 (plugin capabilities as MCP Tool objects), #80
(the cross-repository plan reconciled with Adoption and Settings, which also
carries #78), #81 (docs notes on the canonical form, mergiraf and stale
extraction rows), #82 (the tooling pass: cargo-deny replaces cargo-audit,
actions pinned by SHA, zizmor, cargo-shear, a non-blocking cargo-mutants job),
#83 (gix lock files and `Repository::state()` in `write.rs`) and #84 (the
`records` view with canonical bodies). #55, #60, #74 and #75 are closed. The
tooling pass left out typos, which would need an exception list.

## Open threads

1. **M1, the old-corpus export.** The owner runs it against the installed old
   `board`, into its own repository.
2. **Owner choices on #80.** Whether the share command stays `board publish` or
   returns to `board share`, and whether "Repositories across boards" changes
   from proposed to agreed.
3. **widgets.** The views vocabulary needs read access to example-org/widgets,
   which this session does not have.

## Next actions

1. M7 is the owner's: run the M1 export, time it with
   `cargo xtask bench --board <export>`, choose a real task type (the export
   writes `legacy/*` records, and `tasks` reads `lr/task/v1`), enable `tasks`
   and use Board daily. Route findings to builders as small fixes.
2. Performance baselines: warm check is about 20 µs per entry; three measured
   speedups exist (#33); the chain numbers are in #57 and viitta-stress-test
   9d557d1. Targets (100 ms) are goals; budgets are alarms.
3. Sharing is sketched as M8 in PLAN.md, with network safety and research
   notes; turn it into a design-gap list for a builder after M7.
