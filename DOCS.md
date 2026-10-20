# Docs

- `AGENTS.md`: what Board is and the rules tooling cannot enforce. Every agent loads it.
- `PLAN.md`: what comes next, the deferred sketches, and the decisions that still constrain work.
- `STANDARDS.md`: code standards and the gates that enforce them.
- `docs/architecture.md`: boards, records, paths, links and mounts, the index, writes, codes, testing and performance.
- `docs/plugins.md`: the plugin contract and how plugins are written.
- `controls/`: the rules above and the `lint.toml` word budgets as `controls/control/v1` records. The records only describe; `cargo xtask lint` enforces the budgets and fails when a record disagrees with `lint.toml`.
- `lint.toml`: the documentation, plan, file-length and `#[expect]` budgets.
- `released.json`: the schema files, report fields and codes the last release shipped, which change only by a new version.
