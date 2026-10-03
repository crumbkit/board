# Handoff: vision and review session

State of the session that designed this seed (claude.ai session
`session_01YCLTnYNfyNqGPM5UDViA5C`, "Board architecture rethink"), written
2026-10-03 so that a fresh session can take over. Delete this file once its
open threads are settled or moved into PLAN.md.

## Roles

- **Owner** decides: scope, budgets, repositories, merges.
- **Vision and review session** (the restarted one): holds the design, keeps it
  lean, reviews every builder PR against STANDARDS.md and docs/, and changes
  the docs when the design is wrong. It does not implement milestones.
- **Builder session** `session_018fcBgd866UNNdTiVtbJpFc` ("Session task
  coordination", with crumbkit/board and crumbkit/viitta): was commissioned
  for M2, then told to hold, because the rebuild moves to crumbkit/viitta. It
  confirmed it had started nothing (no branch, commits or files) and is holding.
  Re-brief it before it resumes, with paths relative to viitta's root (no
  `next/` prefix).

## Where things are

- The seed was designed as `next/` on crumbkit/board, branch
  `claude/board-architecture-rethink-v4pwav`, and copied to this repository's
  root by the restarted session. crumbkit/viitta was empty, so its first
  commit is an empty one on `main` and the seed arrives as a pull request.
  Paths here are relative to this root.
- Verified in the board repository: `bin/lint` and `bin/test` pass (37 tests);
  `cargo deny check bans licenses sources` passes, with three expected
  `unused-wrapper` warnings until the owner crates exist. Each guard refused a
  throwaway mutant: `std::fs::write` and `unwrap` in production, `#[allow]`, an
  extra `#[expect]`, and a non-owner crate depending on a wrapped one. The board
  repo's own `scripts/build fmt`, `check` and `clippy` pass; `test` was still
  running at handoff. Not run: `cargo audit`, and anything on macOS.

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

1. **The owner decisions in PLAN.md.** None answered yet.
2. **Budget reading.** `context.max_words` counts the default-loaded files
   together, each once. The owner may have meant per file; with `CLAUDE.md`
   being one import line, the two readings are nearly the same.
3. **proptest and bolero.** Proposed: proptest for invariants over generated
   structures (boards, links, mounts, warm-equals-cold), where its strategies
   and shrinking are strongest; bolero for parsers of untrusted bytes, because
   one target runs as a quick randomised test on stable in the gate and as
   coverage-guided libFuzzer on nightly in a scheduled job, so the fuzz target
   cannot rot. That would replace "cargo-fuzz" in STANDARDS.md rule 19. Not yet
   applied; the owner was asked.
4. **Web UI and sharing: design for it, don't build it.** Proposed positions,
   not yet in PLAN.md:
   - Every operation is a typed library function returning its report. The CLI
     is one adapter; a later `board serve` is another. Put no logic in
     `board-cli`.
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
     widgets's UI. Derive the component vocabulary from that UI, which this
     session could not see (example-org/widgets was outside its access).
5. **Moving to viitta.** Copy the seed, check that CI runs there, then decide
   what happens to the board branch.

## Next actions for the restarted session

1. Read this file, then AGENTS.md, PLAN.md, STANDARDS.md and docs/, in full.
2. Look at crumbkit/viitta and open a PR that brings the seed in.
3. Ask the owner the open questions, in one message, smallest first.
4. Fold accepted answers into PLAN.md, docs/ or STANDARDS.md within budget;
   leave the rest here.
5. Re-brief the builder for M2 in viitta, then review its PRs.
