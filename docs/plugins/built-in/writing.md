# Writing a built-in plugin

This file holds detail for docs/plugins/built-in.md. `controls`, `evals`,
`plans`, `requests` and `runs` are built-in crates; `controls` (#48) shows a
Python plugin ported with its output, messages and codes unchanged.

## What a built-in plugin can do

A built-in plugin reads and writes the board through `Host` and nothing
else: `query`, `refs`, `get_each`, `put` and `settings`
(docs/plugins/built-in.md). It writes records only with `put`, which takes
the lines a Python plugin gives `board put --batch`, and maps its error with
`PluginError::board_refused(<name>, "put --batch", &error.refusal())`. It
cannot yet call another plugin, start a process, read an environment
variable, write a file outside a put, or stop at a deadline. Clippy's
`disallowed-methods` refuses `std::process::Command::new`, `std::env::var`
and `std::fs::write` outside their owners (STANDARDS.md 7), and a plugin
must not work around a missing method. When a plugin needs one, first add
the method to `Host` and its test in board-ops, in a pull request of its
own, and build the plugin on it.

## The crate

Make `crates/board-plugin-<name>/`:

- `Cargo.toml` depends on what the crate uses of board-model, board-ops,
  board-plugin, schemars, serde and serde_json, with board-scenario and
  board-validate as dev-dependencies, and sets `[lints] workspace = true`.
  `cargo shear` fails on an unused dependency.
- `src/lib.rs` holds the plugin, `src/records.rs` its record types, and one
  module per capability, such as `src/thread.rs`.
- `types/<name>/<kind>/v<n>.schema.json` holds each record schema,
  committed. A type's namespace is the plugin's name.

Then wire it in:

1. Add the crate to `members` and `[workspace.dependencies]` in the root
   `Cargo.toml`.
2. In `crates/board-plugins/Cargo.toml`, add the feature
   `<name> = ["dep:board-plugin-<name>"]` and the optional dependency; in
   `src/lib.rs`, add `&board_plugin_<name>::<Plugin>` to `BUILT_IN` under
   `#[cfg(feature = "<name>")]`.
3. In `crates/board-cli/Cargo.toml`, add `<name>` to `default` and
   `<name> = ["board-plugins/<name>"]`.
4. Name the plugin in docs/plugins/built-in.md, in docs/plugins.md and in
   the crate table of docs/architecture.md.

## Records

Each record type is a struct deriving `JsonSchema` with
`#[serde(deny_unknown_fields)]`, whose first field is
`#[serde(rename = "$type")] pub tag: Tag<Self>`, and which implements
`board_plugin::shape::Record` with `const TYPE: &'static str =
"<name>/<kind>/v1"`. Doc comments become the descriptions. A reference is a
`Ref<T>`, and each string field uses a shared shape from
`board_plugin::shape` or `#[schemars(with = "Prose")]`
(docs/architecture/records/shapes.md). `xtask/tests/shapes.rs` checks every
committed schema; a new plugin adds no exemption to it. `schemas()` returns
each type's `RecordType`, parsed from `T::TYPE`, with
`schema_of::<T>(SchemaSettings::draft2020_12())`.

## The plugin

`lib.rs` defines a unit struct and implements `board_ops::Plugin`:

- `manifest()` returns `Manifest::built_in(json!({...}))`, with `$type`,
  `name`, `version`, `description`, `metadata`, `types` and `capabilities`
  and no `run`. `built_in` returns a `Result`, which the plugin `expect`s
  because a test parses the manifest. Each capability's `inputSchema` is
  `schema_of::<Input>(SchemaSettings::draft2020_12())` and its
  `outputSchema` is
  `schema_of::<Output>(SchemaSettings::draft2020_12().for_serialize())`.
- `call()` matches the capability's name and calls its module. Any other
  name is refused with `<name>::call::unknown_capability`, naming what the
  plugin offers.

Each capability module defines `Input` (`Deserialize`, `JsonSchema`,
`deny_unknown_fields`) and `Output` (`Serialize`, `JsonSchema`), renamed
with `#[schemars(rename = "<Capability>Input")]` so schema names stay
unique. It reads its input with `PluginError::read_input`. A `Host` error
becomes `<name>::board::refused` through
`PluginError::board_refused(<name>, <command>, &error.refusal())`. Its own
refusals are `PluginError` values with a `<name>::<area>::<reason>` code. A
path input carries
`#[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]`, so it
may be given relative to the run's prefix (docs/plugins.md, Calling a
capability).

A plugin's skill, README and any Claude Code mod live in `plugins/<name>/`,
as for `controls` and `evals`. When it has a skill, the manifest's
`description` equals the skill's frontmatter `description`, and a test reads
the skill with `include_str!` and compares them.

## Tests

The crate's `src/tests.rs` proves that:

- the manifest parses as Board reads a built-in one, with its name,
  version, types and capabilities;
- each committed schema is what its struct generates, and
  `SNAPSHOTS=overwrite` rewrites the file;
- each schema refuses a bad record and admits its otherwise identical twin;
- an unknown capability is refused with its code, and a known one reaches
  its own check.

Each capability's tests run through a real board. They build a fixture with
`board_scenario::Scenario`, open it with `Board::open`, call
`build_in(&[&<Plugin>])` and `plugin_enable`, then call
`board.call(&target, input.as_bytes(), &Plugins::default())` and assert that
`take_trace().work` counts no `started` process. A test that counts the
work a capability does uses a recording `Host`, as
`board-plugin-controls/src/check/tests.rs` does. In board-cli,
`src/tests/built_in.rs` calls each capability through the binary and shows
that it answers the same from a warm index as from a cold one (STANDARDS.md
19), and the help goldens under `src/tests/help/examples/` list the plugin
in `plugin list` and `plugin find`.

## Porting a Python plugin

- Keep the schema's bytes. A board that already enables the plugin then
  needs no `plugin enable`. When `schema_of` cannot produce the old bytes,
  write the type's own generator, as `controls`'s `record_schema` does to
  keep null in `rule`.
- Keep each capability's output, messages and codes. Drop codes that only a
  subprocess could give, such as `<name>::board::failed`.
- Keep the Python tests as a parity suite that drives the real binary until
  Rust tests cover what they prove, then delete them with the plugin's
  `pyproject.toml` and `uv.lock`.
- Delete `plugin.json`, `src/` and `types/`, and take the plugin out of the
  packaging loop in `.github/workflows/release.yml`.
- Search the repository for the plugin's name and `plugins/<name>`: CI
  steps, the timing bench, `xtask/tests/shapes.rs` and STANDARDS.md. When
  this repository's board enables the plugin, run `board plugin enable
  <name>` so `.board/types/` holds the shipped schema.
- Say the net production lines for Rust and for Python in the pull request.
