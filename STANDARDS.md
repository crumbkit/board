# Standards

Rules 1 to 27 cover the Rust workspace; P1 to P6 cover Python. Each rule says
what enforces it. A rule without a gate is still a rule.

## Types

1. **Parse once at the boundary.** Convert input to its canonical type where it enters, and pass that type inward. A `.to_string()` that is parsed again further in is a defect. Use enums, not `bool`, `Option<Option<T>>` or `String`. Keep malformed, absent and unsupported values distinct.
2. **A newtype holds an invariant.** Add one only to hold an invariant, to tell apart values that are easy to confuse, or to own a shared rule. Keep the field private. Implement `FromStr` and `TryFrom<String>`, with `#[serde(try_from = "String", into = "String")]`. Constructors and deserializers enforce the same invariant. Wire-only structs are named `Raw*`.
3. **Board's own documents refuse what they do not understand.** Every `board/*` document (configuration, pointers, manifests, plugin messages, reports) uses `#[serde(deny_unknown_fields)]`, refuses duplicate keys and carries `"$type": "board/<kind>/v<n>"`. Never change a released schema; add a version. User records are different: their own schema decides, `put` refuses an invalid one, and the index marks it degraded instead of refusing it. serde's derived structs refuse duplicate fields, but `serde_json::Value` and maps keep the last one silently, so every untyped JSON read goes through `board-model`'s duplicate-refusing parse.

## Errors

4. **Every refusal carries a stable code.** Each owning module has one `thiserror` enum. Each variant has a code `<app>::<area>::<reason>`, spelled once, which never changes after release. The area is a stable domain noun (`mount`, `record`, `put`), never a module path, so moving code never changes a code. Reasons are snake_case. The message is one lower-case sentence that names the value at fault. Store typed fields, never a preformatted `String` or `Debug` output. A blanket `#[from] io::Error` is a fault. A degraded entry is a path and one of these codes.
5. **`anyhow` only in a binary's `main`, if at all.** Library crates return typed errors.
6. **Production code does not panic.** No `unwrap`, `dbg!`, `todo!`, `unimplemented!` or `panic!`. Use `expect` only for a proven invariant, and state the invariant in its message. Recover a poisoned lock through a helper, never `.lock().unwrap()`. Tests may unwrap, expect and panic on a broken fixture. Gate: `unwrap_used`, `dbg_macro`, `todo`, `unimplemented` and `panic` deny; `clippy.toml` sets `allow-unwrap-in-tests`, `allow-expect-in-tests` and `allow-panic-in-tests`.

## Structure

7. **One owner for each effect.** The environment, child processes, signal handlers, file writes, locks and stdout/stderr each have one owning module. Signal handlers belong to `board-plugin`'s `process.rs`, which forwards stop signals to a plugin's process group; no other module mutates process-global state. Read configuration once at the boundary. A library that performs an effect has one owning crate: only `board-tree` depends on gix, only `board-index` on rusqlite, only `board-validate` on jsonschema. Tests write fixture files through one test-support function. Gate: clippy `disallowed-methods`, with a reason that names the owner; cargo-deny `wrappers` for crates. When you change the lint wiring, add a forbidden call and confirm that the gate refuses it.
8. **Decisions are pure; effects can be replaced.** Decision code does no I/O. Put an effect behind a narrow trait with a recording fake only when a test needs it; do not add a trait for every helper. Split crates on this line. Production code has no `#[cfg(test)]` branch.
9. **Narrow visibility.** Write `pub(crate)` by default. A `pub` item inside a private module is a mistake. Gate: `unreachable_pub`.
10. **Module layout.** Use `foo.rs` beside a `foo/` directory, never `mod.rs`. Import production items by name; use `use super::*` only in tests. Put large tests in a sibling `tests.rs`. Gate: `mod_module_files`, `wildcard_imports`.
11. **`unsafe` is forbidden.** Set `unsafe_code = "forbid"`. Where a platform needs `unsafe`, isolate it in one crate with `unsafe_op_in_unsafe_fn = "deny"`. Put a `SAFETY` comment on each block and a `# Safety` section on each `unsafe fn`. Gate: `undocumented_unsafe_blocks` deny; `missing_safety_doc` through `-D warnings`.
12. **Suppress with `#[expect]`, never `#[allow]`.** Write `#[expect(lint, reason = "...")]` on the smallest item that needs it. On code only some platforms compile, write `#[cfg_attr(target_os = "...", expect(...))]`, or the expectation goes unfulfilled elsewhere and fails `-D warnings`. Gate: `allow_attributes` and `allow_attributes_without_reason` deny. `cargo xtask lint` counts the `#[expect]` attributes in production source and fails unless the count equals `expect.count` in `lint.toml`: adding one raises the number in the same change, removing one lowers it.
13. **No guard that nobody asked for.** Add a defensive check or a gate only for a failure that has happened or, for untrusted input such as a mounted board, a known class of attack: huge files, deep nesting, cycles, pathological patterns. A refactor deletes code; report the net change in production lines.
14. **Functions stay under 100 lines; files under 800.** Gate: `too_many_lines` through `pedantic`; each function over the limit carries its own `#[expect(clippy::too_many_lines, reason = "...")]`. `cargo xtask lint` fails a production source file over `source.max_lines`; `tests.rs` files and `tests/` directories are exempt. This limit passes rule 13: the old Board's `objects.rs` reached 11,000 lines.
15. **A comment says why, and only when it constrains.** Prefer names and tests. Keep a comment for an upstream quirk, a wire requirement or a measured figure. Do not narrate history. Write a doc comment only when the signature does not already say it. Put notes for agents in `AGENTS.md`, within its budget (rule 22).
16. **Secrets are never printed.** Wrap secrets in `secrecy`. Write a manual `Debug` that redacts them. Compare credentials in constant time.

## Interfaces

17. **A command returns one typed report.** A handler returns one `Serialize` report, and one module prints it. Exit codes are 0, 1 and 2 (2: the command line was refused). `--json` writes a document tagged `board/<command>-report/v1`, and `board/error/v1` on failure. A command has no aliases. A read or check reports every entry even after one fails; a write changes everything it was asked to or nothing. `board check` exits 1 when any entry is degraded, so it works as a gate. `board refs` and `board get` exit 1 when any path they were asked is refused, and still print their report; a path left past the run's byte budget is refused in its place. `board query` cut at the byte budget exits 0, because it was asked for no path that it refused. Repository tooling follows this rule except `--json`, which it gains when something reads it. Gate: `print_stdout` and `print_stderr` deny.
18. **No async runtime.** Board is a local, synchronous CLI, and gix and rusqlite are synchronous. A dependency that brings in an async runtime states why in the change that adds it.

## Tests

19. **Tests come first and describe behaviour.** Keep unit tests inline or in a sibling `tests.rs`, and integration tests in `tests/`. Name each test as a sentence about behaviour. Test against real local dependencies: git repositories in temporary directories, SQLite, child processes. Pair every refusal test with an otherwise identical case that is admitted, and assert the exact code. Test a rule as a pure function over many generated cases (`proptest`). Integration tests prove the wiring, a few for each seam. Keep a property test over real repositories only for an invariant that crosses a seam: one at the index snapshot shows that a warm index decides as a cold one, and each command that reads the index has a short example that answers the same warm as cold. Freeze wire formats and canonical output with golden bytes. Fuzz every parser that reads untrusted input with a `bolero` target: it runs as a randomised test in the gate and as coverage-guided fuzzing in a scheduled job, so the target cannot rot. Nextest uses `retries = 0` (a flaky test is a defect) and `fail-fast = false`. Run doctests separately.

## Performance

20. **Count work, not time, in the gate.** A work-count test asserts how much work an operation does: after one record changes, a refresh parses exactly one blob; resolving through `n` pointers reads at most `n` pointer records. Such tests never flake, and they catch work that should grow with the change but grows with the corpus. One timing job runs the reference boards against the budgets in `docs/architecture.md`, reports every run and fails above three times a budget. A bound on what an agent receives (rows, bytes) stays small and explicit; a bound that only sizes Board's own work is set for realistic growth.

## Toolchain and gates

21. **The same gates everywhere.**
    - Use edition 2024 and the default rustfmt settings.
    - Pin the toolchain in `rust-toolchain.toml`, and state the reason when it holds back a version.
    - Declare `[workspace.lints]`, and set `[lints] workspace = true` in every member.
    - `bin/lint` runs `cargo fmt --all --check`, then `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, then `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked` (most items are `pub(crate)`, so without the flag their docs go unchecked), then `cargo xtask lint`.
    - `bin/test` runs nextest with the profile in `.config/nextest.toml`, then doctests.
    - CI runs both scripts on Linux for each pull request and push to `main`; `release.yml` reruns them on macOS and Linux, with the timing job, before a release. CI also runs `cargo deny check`, `cargo shear` and `zizmor` on the workflows. Every action is pinned by commit SHA and every installed tool by exact version. A non-blocking `cargo mutants --in-diff` job reports on each pull request.

## Documentation

22. **Four word budgets, each set on its own in `lint.toml`; the docs budget applies to each directory.** `cargo xtask lint` counts whitespace-separated words, code blocks included; for English prose a word is roughly 1.3 to 1.5 tokens.

    | Budget | Covers | Start | Why |
    | --- | --- | --- | --- |
    | N, `context.max_words` | `AGENTS.md`, `CLAUDE.md` and every file they import with `@path`, each counted once | 1,000 | Every agent session pays it, so it holds orientation and the rules tooling cannot enforce: about 1,400 tokens, four minutes to read. The old `AGENTS.md` alone was 5,879 words. |
    | M, `docs_index.max_words` | `DOCS.md` | 400 | A map, one line per document. If it does not fit, `docs/` has too many files. |
    | O, `docs.max_words` | each directory node under `docs/`: the `.md` files directly inside it, summed | 5,500 | What one person reads in one sitting, about 25 minutes. Past that, knowledge belongs in code, tests or schemas, or the node needs a level below it. The owner set 5,000 after the altitude pass and 5,500 to describe the features built since. The old `docs/` held about 103,000 words. |
    | P, `plan.max_words` | `PLAN.md` | 1,075 | The plan holds only what comes next. It had grown to 7,377 words, more than the docs budget, mostly sketches of unbuilt features; it was cut to what is next and a one-line list of deferred sketches. |

    `docs/` and every directory below it is a node, and O applies to each node on its own. A subdirectory's words count toward its own node and never toward its parent. When a node is full, detail moves one level deeper: a file such as `docs/architecture.md` links once to a leaf file in `docs/architecture/`, which holds the detail, and keeps a short summary that says what the leaf covers. No rule or value is stated at both levels. `DOCS.md` lists only the files directly in `docs/`.

    Raise a number only in a change that says why. Every run prints usage against each budget and each docs node, so headroom shows before it runs out. A node over its limit fails with `xtask::budget::over_limit`, which names the node, its words and its limit. Root files that no budget names are outside the budgets. `PLAN.md` shrinks as milestones land, and an unbuilt sketch gets one line there and a pointer to the commit that holds its full text. Each budget is also recorded as a control, `controls/budgets/<section>.json`. The control does not enforce anything. `cargo xtask lint` fails when a budget has no control, when a control has no budget, when a control's `applies_to` and `paths` name other files than the section, when the only number in its `requirement` is not the section's `max_words`, or when it carries a `rule`. The docs section names `docs/*.md`, a SQLite GLOB in which `*` also matches `/`, so its one control covers every node.

## Command line

AGENTS.md 9 sets the goal: the CLI serves agents and people equally. These rules
make it concrete. Rule 17 already fixes the report, the exit codes and `--json`.

23. **Output is bounded and says what it left out.** A command that lists entries
    (`check`, `ls`, `refs`, `plugin list`, `plugin find`, `query`) shows at most
    100 by default. `--limit <n>` (1 to 10,000) or `--all` changes that. When
    entries are left out, text output ends with a note on stderr that gives the
    count shown, the total and the flag that shows more. JSON carries `total` and
    `truncated`. `query` gives no total, because it does not fetch the rows it
    leaves out. `check` lists degraded
    entries first, so a cut never hides a problem behind healthy entries. `refs`
    limits each path's answer on its own, so the caller bounds a run by how many
    paths it asks. `get` and `refs` stop answering once their output reaches
    64 MiB and refuse each later path as `get::over_budget` or
    `refs::over_budget`, to ask for in another run. `query` stops before the
    row that would take its rows past 64 MiB, even with `--all`, and marks the
    cut `query::over_budget`; it refuses a first row or a single value longer than
    that, where `get` and `refs` always answer their first path, because a
    query can ask for fewer or smaller columns and a file has no smaller form.
    An empty listing says so on stderr instead of printing nothing. Gate: a test
    walks the clap command tree and requires every listing command to take these
    flags.
24. **Help is a tree that ends in examples.** `board --help` gives one line per
    command and the commands a newcomer runs first. `board <command> --help`
    describes every argument and option, and ends with at least one example.
    Gate: a test walks the clap command tree and requires help text on every
    command, argument and option, and examples on every command that runs.
    Every example in help must parse as a real command line.
25. **Every error says what failed, for which value, and what to try next.** A
    refused command line is an error like any other: it has the code
    `board::usage::invalid`, exit code 2, and a `board/error/v1` document when
    `--json` is on the command line. clap suggests the nearest command or flag
    for a misspelling. Text errors are `error: <code>: <message>`, then a
    `hint:` line. JSON errors carry `hint` beside `code` and `message`. A record
    refused by its schema names the JSON pointer that failed and why. Gate: the
    hint for each code is chosen by an exhaustive `match` on `Code`, so a new
    code does not compile until it has a hint, and a test requires every command
    a hint names to parse. A property provokes every code it can through the
    command line and runs each command a hint names, up to three hops; a
    command that fails again with the code that named it is a dead end, except
    `check`, which reports what is still degraded. A `check` hint that says to
    fix something, then run that same `check`, ends its chain. The property
    accepts the placeholders `<dir>` and `<command>`, which only the reader can
    fill in. Any other placeholder, such as `<path>`, belongs to a generic
    hint for an error that lacks the value, and the property counts it as a
    dead end. A plugin's own codes carry the plugin's message and no hint from
    Board.
26. **Output does not depend on the terminal.** Board prints the same bytes to a
    terminal as to a pipe. It does not use colour, wrap lines or detect a
    terminal. The report goes to stdout. Notes, hints and text errors go to
    stderr.

## Call order

27. **Types enforce a fixed call order.** When calls must happen in a fixed
    order, each step takes ownership of the value the previous step returned
    and returns the value the next step needs, so a call out of order does not
    compile. A step that needs an earlier value to stay held, such as a lock,
    borrows it instead. In `board-index`, `Index::write` returns a `Writer`,
    `Writer::decide` consumes it with the entries the run decided, commits
    and returns a `Decided` holding them, `Decided::view` consumes that, fills
    the views with them and returns a `View`, and only a `View` reads them;
    dropping a `Writer` rolls back. In
    `board-tree`, `lock` returns a `Lock`, `stage_commit` borrows it and returns
    `Staged`, and `finish_commit` consumes `Staged`.

## Baseline lints

    [workspace.lints.rust]
    unsafe_code = "forbid"
    unreachable_pub = "warn"
    missing_debug_implementations = "warn"

    [workspace.lints.clippy]
    pedantic = { level = "warn", priority = -1 }
    module_name_repetitions = "allow"
    must_use_candidate = "allow"
    missing_errors_doc = "allow"
    dbg_macro = "deny"
    unwrap_used = "deny"
    todo = "deny"
    unimplemented = "deny"
    panic = "deny"
    undocumented_unsafe_blocks = "deny"
    disallowed_methods = "deny"
    disallowed_macros = "deny"
    disallowed_types = "deny"
    allow_attributes = "deny"
    allow_attributes_without_reason = "deny"
    mod_module_files = "deny"
    print_stdout = "deny"
    print_stderr = "deny"

## Python: plugins

- **P1. Toolchain.** Python 3.12 or newer, managed with uv. Commit the lockfile; CI installs with `uv sync --frozen`.
- **P2. Gates.** `ruff format --check`, `ruff check` with every rule selected and each ignored rule listed with its reason, `pyright` in strict mode, and `pytest`. CI runs them on Linux, and on macOS before a release.
- **P3. Typed edges.** Board validates a call's input and output, and the records in the board's own tree, against the plugin's schemas (docs/plugins.md), so a plugin types them with TypedDicts and imports pydantic only to generate its schemas. A test compares the committed manifest and schemas with the generated ones, and another that a call loads no pydantic. A plugin that checks more than its schemas state validates with pydantic models with `extra="forbid"`.
- **P4. Board's contract.** Rules 3, 4 and 17 apply to the protocol: every document carries `$type`, every error a stable code `<plugin>::<area>::<reason>`, the exit code is 0 or 1, and logs go to stderr only.
- **P5. Pure extraction.** Extraction is a function of one record's bytes. Anything spanning records is a query.
- **P6. Tests.** Test against real fixture boards and the real protocol. Rule 19 applies: sentences as names, every refusal paired with an admitted twin, no retries.
