# Standards

Rules 1 to 29 cover the Rust workspace; P1 to P5 cover Python. Each rule says
what enforces it. A rule without a gate is still a rule.

## Types

1. **Parse once at the boundary.** Convert input to its canonical type where it enters, and pass that type inward. A `.to_string()` that is parsed again further in is a defect. Use enums, not `bool`, `Option<Option<T>>` or `String`. Keep malformed, absent and unsupported values distinct.
2. **A newtype holds an invariant.** Add one only to hold an invariant, to tell apart values that are easy to confuse, or to own a shared rule. Keep the field private. Implement `FromStr` and `TryFrom<String>`, with `#[serde(try_from = "String", into = "String")]`. Constructors and deserializers enforce the same invariant. Wire-only structs are named `Raw*`.
3. **Board's own documents refuse what they do not understand.** Every `board/*` document (configuration, pointers, manifests, plugin messages) uses `#[serde(deny_unknown_fields)]`, refuses duplicate keys and carries `"$type": "board/<kind>/v<n>"`. A command's report and its failures are crumbkit documents named by `schema` instead (rule 17). User records are different: their own schema decides, `put` refuses an invalid one, and the index marks it degraded instead of refusing it. serde's derived structs refuse duplicate fields, but `serde_json::Value` and maps keep the last one silently, so every untyped JSON read goes through `board-model`'s duplicate-refusing parse.

## Errors

4. **Every refusal carries a stable code.** Each owning module has one `thiserror` enum. Each variant has a code `<app>::<area>::<reason>`, spelled once. The area is a stable domain noun (`mount`, `record`, `put`), never a module path, so moving code never changes a code. Reasons are snake_case. The message is one lower-case sentence that names the value at fault. Store typed fields, never a preformatted `String` or `Debug` output. A blanket `#[from] io::Error` is a fault. A degraded entry is a path and one of these codes.

Rules 3 and 4 on change: after a release, a changed type is a new version and a code is never renamed; until a release or a second user, both change in place and every reader on the machine moves in the same step. Gate: `released.json` lists the released schema files, report fields and codes, and the `released` test in `board-cli` refuses an in-place change to any of them (docs/architecture.md, Released interfaces).

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

17. **A command returns one typed report.** A handler returns one `Serialize` report, and one module prints it. Exit codes are 0, 1 and 2 (2: the command line was refused). `--json` writes crumbkit's envelope: `schema` `board/<command>-report/v2`, the report as `data`, and a `board/failure/v1` document in `failures` for each refused path, with its `key` and `index`. A listing adds `total`, `truncated` and `summary`. A failed command prints one `board/failure/v1` document. Notes, such as an index held in memory, go to stderr in both formats. A built-in command has no aliases; a plugin's manifest alias (docs/plugins.md, Aliases) is the one way a word other than a built-in command runs. A read or check reports every entry even after one fails; a write changes everything it was asked to or nothing. One exception: a sync that finds a file edited during its push keeps the edit and lists it in `kept` with `board::sync::kept`, because the push cannot be undone. `board check` exits 1 when any entry is degraded, so it works as a gate. `board refs` and `board get` exit 1 when any path they were asked is refused, and still print their report; a path left past the run's byte budget is refused in its place. `board query` cut at the byte budget exits 0, because it was asked for no path that it refused. Repository tooling follows this rule except `--json`, which it gains when something reads it. Gate: `print_stdout` and `print_stderr` deny.
18. **No async runtime.** Board is a local, synchronous CLI, and gix and rusqlite are synchronous. A dependency that brings in an async runtime states why in the change that adds it.

## Tests

19. **Tests come first and describe behaviour.** Keep unit tests inline or in a sibling `tests.rs`, and integration tests in `tests/`. Name each test as a sentence about behaviour. Test against real local dependencies: git repositories in temporary directories, SQLite, child processes. Pair every refusal test with an otherwise identical case that is admitted, and assert the exact code. Test a rule as a pure function over many generated cases (`proptest`). Integration tests prove the wiring, a few for each seam. Keep a property test over real repositories only for an invariant that crosses a seam: one at the index snapshot shows that a warm index decides as a cold one, and each command that reads the index has a short example that answers the same warm as cold. Freeze wire formats and canonical output with golden bytes. Fuzz every parser that reads untrusted input with a `bolero` target: it runs as a randomised test in the gate and as coverage-guided fuzzing in a scheduled job, so the target cannot rot. Nextest uses `retries = 0` (a flaky test is a defect) and `fail-fast = false`. Run doctests separately.

## Performance

20. **Count work, not time, in the gate.** A work-count test asserts how much work an operation does: after one record changes, a refresh parses exactly one blob; resolving through `n` pointers reads at most `n` pointer records. Such tests never flake, and they catch work that should grow with the change but grows with the corpus. One timing job runs the reference boards against the budgets in `docs/architecture/performance/budgets.md`, reports every run and fails above three times a budget. A bound on what an agent receives (rows, bytes) stays small and explicit; a bound that only sizes Board's own work is set for realistic growth.

## Toolchain and gates

21. **The same gates everywhere.**
    - Use edition 2024 and the default rustfmt settings.
    - Pin the toolchain in `rust-toolchain.toml`, and state the reason when it holds back a version.
    - Declare `[workspace.lints]`, and set `[lints] workspace = true` in every member.
    - `bin/lint` runs `cargo fmt --all --check`, then `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, then `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items --locked` (most items are `pub(crate)`, so without the flag their docs go unchecked), then `cargo xtask lint`, then the Python lint gates of P2, then board-ui's type check and build in the pinned Playwright container, which must leave the committed `ui/dist` unchanged. `bin/lint rust`, `bin/lint python` and `bin/lint ui` run one part.
    - `bin/test` runs nextest with the profile in `.config/nextest.toml`, then doctests, then each Python project's `pytest`, the projects at once, then `bin/ui-goldens`, which compares each board-ui story with its golden screenshot in both themes in the same container. `bin/test rust [nextest options]`, `bin/test doc`, `bin/test python` and `bin/test ui` run one part.
    - The pull-request lane, `ci.yml`, runs on Linux for each pull request and push to `main`. It runs the same scripts split across parallel jobs: `lint` (`bin/lint rust`, `bin/test doc`), three `test` jobs that each run a third of nextest's tests, chosen by a hash of the test's name, `python` (`bin/lint python`, `bin/test python`, and `board check`, which reads the repository's controls through the built-in controls plugin), and `ui` (`bin/lint ui`, `bin/test ui`) on an arm64 runner. Each job restores the dependencies' build with `Swatinem/rust-cache`, and only `main` saves it. It also runs `cargo deny check`, `cargo shear` and `zizmor` on the workflows. `mods.yml` runs `claude plugin test` on each `plugins/<name>/claude-code/` mod, checks that `.claude-plugin/marketplace.json` lists every mod, and validates the marketplace. It runs only when a change touches a file under a `claude-code/` directory or the marketplace. The lane takes about a minute and a half when it restores the cache, and about three minutes when it builds the dependencies from scratch. A changed `Cargo.lock` still restores most of the cache. The build starts from scratch after a toolchain bump or a change to a `CARGO*` or `RUST*` variable, after GitHub evicts the cache, and on a pull request that runs before `main` has saved one.
    - The slow lane, `slow.yml`, runs nightly on `main`, by hand, and before each release. It runs both scripts on macOS without the ui part, which needs Docker and which `ci.yml` runs on Linux, fuzzing (rule 19), the timing job (rule 20), and `cargo mutants` over the Rust lines changed since a base commit. Mutants run in two passes: the first tests each mutant with its own crate's tests, and the second tests the mutants the first missed or timed out with the whole workspace's tests. The nightly base is the head of the newest scheduled run of the last 3 days whose mutants all passed, or, with none, the last commit before 00:00 UTC of the previous day; the `base` job's summary names it and says why. A hand-run may give `mutants-base`, which does not move the nightly base. Fuzzing runs every night. Linux timing and mutants are skipped when `main` has not changed since the previous scheduled run whose conclusion is not cancelled, except that a Monday run that times macOS also times Linux. A run in which a job timed out ends cancelled, as a hand-cancelled run does, so the night after either runs Linux timing and mutants again. Of the scheduled runs, only Monday's runs the scripts and the timing job on macOS, which bills ten times Linux. It skips them when `main` has not changed since the previous Monday run whose `macos` job passed. After a Monday run whose `macos` job failed or was cancelled, macOS therefore waits for the next Monday and then runs again, even on an unchanged `main`. A hand-run and a release run always run the macOS jobs. A failure that only macOS shows can therefore stay on `main` for up to a week before the slow lane reports it, and a release still checks macOS before it ships. A nightly job that fails or runs past its timeout, including a missed mutant, opens an issue titled "Slow lane failed", and later failures comment on it until someone closes it; a run cancelled by hand reports nothing. A missed mutant is fixed by the test that catches it. One nobody fixes is reported on each night `main` changes, until the newest run whose mutants all passed is more than 3 days old. After that the base falls back to the previous day, so each change is mutated on one or two nights only. A shard that runs past its timeout never passes, so the 3-day window bounds a failing streak's range to about 4 days of changes.
    - `release.yml` runs both lanes, then builds `board` for aarch64-apple-darwin, aarch64-unknown-linux-gnu and x86_64-unknown-linux-gnu, packages each plugin, runs `bin/install-check` on each platform, and attaches the archives and `SHA256SUMS` to the release the tag `v<version>` names; the tag must equal board-cli's version, which `board --version` prints. A release runs the slow lane without mutants, because the nightly runs have mutated each day's changes.
    - Every action is pinned by commit SHA and every installed tool by exact version.

## Documentation

22. **Five word budgets, each set on its own in `lint.toml`; the docs budget applies to each directory and the plan-leaves budget to each leaf.** `cargo xtask lint` counts whitespace-separated words, code blocks included; for English prose a word is roughly 1.3 to 1.5 tokens.

    | Budget | Covers | Start | Why |
    | --- | --- | --- | --- |
    | N, `context.max_words` | `AGENTS.md`, `CLAUDE.md` and every file they import with `@path`, each counted once | 1,000 | Every agent session pays it, so it holds orientation and the rules tooling cannot enforce: about 1,400 tokens, four minutes to read. The old `AGENTS.md` alone was 5,879 words. |
    | M, `docs_index.max_words` | `DOCS.md` | 400 | A map, one line per document. If it does not fit, `docs/` has too many files. |
    | O, `docs.max_words` | each directory node under `docs/`: the `.md` files directly inside it, summed | 5,500 | What one person reads in one sitting, about 25 minutes. Past that, knowledge belongs in code, tests or schemas, or the node needs a level below it. The owner set 5,000 after the altitude pass and 5,500 to describe the features built since. The old `docs/` held about 103,000 words. |
    | P, `plan.max_words` | `PLAN.md` | 1,075 | The plan holds only what comes next. It had grown to 7,377 words, more than the docs budget, mostly sketches of unbuilt features; it was cut to what is next and a one-line list of deferred sketches. |
    | Q, `plan_leaves.max_words` | each milestone leaf, a `.md` file directly in `plan/` | 1,300 | A milestone whose design does not fit in `PLAN.md` keeps it in one leaf. Set at the first leaf's length plus about 15%. |

    `docs/` and every directory below it is a node, and O applies to each node on its own. A subdirectory's words count toward its own node and never toward its parent. When a node is full, detail moves one level deeper: a file such as `docs/architecture.md` links once to a leaf file in `docs/architecture/`, which holds the detail, and keeps a short summary that says what the leaf covers. No rule or value is stated at both levels. `DOCS.md` lists only the files directly in `docs/`.

    Raise a number only in a change that says why. Every run prints usage against each budget and each docs node, so headroom shows before it runs out. A node over its limit fails with `xtask::budget::over_limit`, which names the node, its words and its limit. Root files that no budget names are outside the budgets. `PLAN.md` shrinks as milestones land, and an unbuilt sketch gets one line there and a pointer to the commit that holds its full text. The plan overflows by milestone, not by depth: `plan/` is flat, holds one leaf per milestone, and `PLAN.md` links each leaf once. A milestone too big for one leaf is split into two milestones. Landing a milestone deletes its leaf in the change that moves its design into `docs/`. `cargo xtask lint` fails with `xtask::plan::index_differs` when a leaf is not linked, is linked twice, a link names a missing leaf, or `plan/` holds a subdirectory. Each budget is also recorded as a control, `controls/budgets/<section>.json`. The control does not enforce anything. `cargo xtask lint` fails when a budget has no control, when a control has no budget, when a control's `applies_to` and `paths` name other files than the section, when the only number in its `requirement` is not the section's `max_words`, or when it carries a `rule`. The docs section names `docs/*.md`, a SQLite GLOB in which `*` also matches `/`, so its one control covers every node.

## Command line

AGENTS.md 9 sets the goal: the CLI serves agents and people equally. These rules
make it concrete. Rule 17 already fixes the report, the exit codes and `--json`.

23. **Output is bounded and says what it left out.** A command that lists entries
    (`check`, `ls`, `refs`, `plugin list`, `plugin find`, `query`) shows at most
    100 by default. `--limit <n>` (1 to 10,000) or `--all` changes that. When
    entries are left out, text output prints a note on stderr that gives the
    count shown, the total and the flag that shows more. JSON carries `total` and
    `truncated`. `query` gives no total, because it does not fetch the rows it
    leaves out. `check` lists degraded
    entries first, so a cut never hides a problem behind healthy entries. `refs`
    limits each path's answer on its own, so the caller bounds a run by how many
    paths it asks. `get` and `refs` stop answering once their output reaches
    64 MiB and name every later path in the `left_out` of one
    `board::output::over_budget` failure, to ask for in another
    run. `query` stops before the
    row that would take its rows past 64 MiB, even with `--all`, and marks the
    cut with the same code; it refuses a first row or a single value longer than
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
    `board::cli::usage`, exit code 2, and a `board/failure/v1` document
    when `--json` is on the command line. clap suggests the nearest command or
    flag for a misspelling. Text errors are `error: <message>`, then clap's
    `usage:` line when clap prints one, then `help:` and `code:` lines. One
    exception: in text, a command group run without a subcommand prints the
    group's help page on stderr and exits 2, because exit 2 already means
    `board::cli::usage`; with `--json` it prints the failure document. JSON errors carry `recovery` beside `code` and
    `message`. A record
    refused by its schema names the JSON pointer that failed and why. Gate: the
    hint for each code is chosen by an exhaustive `match` on `Code`, so a new
    code does not compile until it has a hint, and a test requires every command
    a hint names to parse. crumbkit-test's recovery check provokes every code
    it can through the command line and runs each command a hint names, up to
    three hops. A command that fails again with a code earlier in its chain
    fails the check, except `check`, which reports what is still degraded. No
    chain runs a command line twice. A chain may come back only to a `check`
    that a hint says to run after a fix ("then run `board check ...`"), and it
    ends there. The check accepts
    the placeholders `<dir>` and `<command>`, which only the reader can fill
    in. Any other placeholder, such as `<path>`, belongs to a generic hint for
    an error that lacks the value, and fails the check. crumbkit-test's twin
    check runs, for each code, a refused line beside a line of the same
    command one word apart, or without one variable, that succeeds. A code
    that no word or variable causes is listed with its reason and the test
    that pairs it. A listing's entries, such as `plugin list`'s, report each
    one's problem as its status and carry no hint. A plugin's own codes carry the plugin's message and no hint from
    Board.
26. **Output does not depend on the terminal.** Board prints the same bytes to a
    terminal as to a pipe. It does not use colour, wrap lines or detect a
    terminal on stdout or stderr. `board call` takes a terminal on stdin as no
    input, and does not read stdin for a capability whose schema admits only
    flag properties, or for a target nobody offers, so it does not wait for input nobody will type. The report goes to stdout. Notes, hints and text errors go to
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

## Launch codes

28. **A local web app is opened with a one-time launch code, never its
    token.** A command that opens a browser on a local server passes a code
    that the page exchanges once for the server's token. Gate: the server's
    tests pair each refusal with an admitted twin.
    - The code is at least 128 bits from the operating system's random
      source and is compared in constant time, so it cannot be guessed or
      timed.
    - It is single use and consumed atomically, so two racing exchanges
      cannot both succeed.
    - It expires within 60 seconds, and the server forgets it on use or
      expiry, keeping at most a digest to tell used and expired codes from
      unknown ones. A code found later is worthless.
    - It travels only in the URL fragment, never the query, so it stays out
      of requests, server logs and `Referer` headers. Browser history may
      keep it, which is why it is single use.
    - It never goes in a process argument or environment variable, which
      other users can read. It reaches the browser through a file only its
      owner can read.
    - The page exchanges it by `POST` with the code in the body, because
      link previews and prefetching send `GET`s. The reply carries
      `Cache-Control: no-store`, so no cache keeps the token.
    - The page holds the token only in memory; no cookie or storage holds
      it, so nothing durable leaks it.
    - Whichever process writes a file holding the code deletes it once the
      code is used or expired, so no copy on disk outlives it.
    - Refusals carry distinct codes for an unknown, a used and an expired
      code, so the reader knows whether to relaunch or investigate.

## Signing

29. **Signing has one owner.** `board_model::effect` builds the statements
    and names their namespaces. `board_plugin::sign` runs the signer and
    `ssh-keygen` and checks the allowed signers file. Only board-ops's
    effects module calls `sign`, `verify`, `Effect::approval` or
    `Effect::owner_write`. A changed layout is a new `vectors/signing/`
    version (docs/architecture/writes/signing.md). Gate: clippy
    `disallowed-methods` on those four functions; `cargo xtask lint` fails
    with `xtask::signing::outside_owner` when source code outside
    `signing.owners` in `lint.toml` names a signing literal; cargo-deny bans
    the crates that sign or verify in process; the `released` test refuses
    an edit to a vector.

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
- **P2. Gates.** `ruff format --check`, `ruff check` with every rule selected and each ignored rule listed with its reason, `pyright` in strict mode, and `pytest`, run in every Python project by `bin/lint` and `bin/test`.
- **P3. Typed edges.** Board validates a call's input and output, and the records in the board's own tree, against the plugin's schemas (docs/plugins.md), so a plugin types them with TypedDicts and imports pydantic only to generate its schemas. A test compares the committed manifest and schemas with the generated ones, and another that a call loads no pydantic. A plugin that checks more than its schemas state validates with pydantic models with `extra="forbid"`.
- **P4. Board's contract.** Rules 3, 4 and 17 apply to the protocol: every document carries `$type`, every error a stable code `<plugin>::<area>::<reason>`, the exit code is 0 or 1, and logs go to stderr only.
- **P5. Tests.** Test against real fixture boards and the real protocol. Rule 19 applies: sentences as names, every refusal paired with an admitted twin, no retries.
