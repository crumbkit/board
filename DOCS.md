# Docs

- `AGENTS.md`: what Board is and the rules tooling cannot enforce. Every agent loads it.
- `PLAN.md`: what comes next, the deferred sketches, and the decisions that still constrain work. `plan/` holds one leaf per milestone too big for it.
- `STANDARDS.md`: code standards and the gates that enforce them.
- `docs/architecture.md`: boards, records, paths, links and mounts, the index, writes, codes, testing and performance.
- `docs/plugins.md`: the plugin contract and how plugins are written.
- `ui/`: board-ui, the web app `board serve` embeds; docs/architecture/ui/serve.md describes it.
- `controls/`: the rules above and the `lint.toml` word budgets as `controls/control/v1` records. The records only describe; `cargo xtask lint` enforces the budgets and fails when a record disagrees with `lint.toml`.
- `lint.toml`: the documentation, plan, file-length and `#[expect]` budgets.
- `released.json`: the schema files, report fields, codes and vector files the last release shipped, which change only by a new version.
- `vectors/`: fixed signing statements, signatures and test keys that Board's tests check; docs/architecture/writes/signing.md describes them.
