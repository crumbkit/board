# params

A plugin built into Board that stores calibrated values. Each value has a
heuristic, which says how an agent derives it, and a value record, which
holds what an agent derived, with its evidence and the date. Plugins read a
value through `params.get` and copy it into no other record. `SKILL.md` says
how an agent reads and recalibrates values.

The evidence is a `runs/run/v1` record, so a board that holds values also
enables the `runs` plugin.

## The heuristic record

A heuristic is a `params/heuristic/v1` record at `/params/heuristics/<name>`.

- `summary` says what the value is for, and `unit` names its unit, such as
  `changed-lines` or `active-days`.
- `inputs` lists each input by `name`, with its `meaning` and its `source`,
  the command that measures it.
- `method` says how to derive the value from the inputs. `start` is the
  estimate the method starts from.
- `default` is the lenient value readers use without enough data. `enough`
  says what counts as enough data.
- `bounds` holds `min` and `max`. Readers do not use a value outside them.
- `check` holds a `measure`, with the command that computes it, and
  `fails_above`. When the measure is greater than `fails_above`, the value
  is wrong.
- `recheck_after` is the number of days after which the value is due again.
  They are calendar days until the frecency change switches them to active
  days. `consumer` names the plugin that reads the value.

The plugin compiles two heuristics into Board, `overflow-agent-grace` and
`overflow-owner-grace`, from `crates/board-plugin-params/heuristics/` in
Board's repository, so every board that enables the plugin has them. Each
scales a starting grace of changed lines by how busy a repository is against
the median of the board's repositories, and writes a value per repository
under `scopes`.

To change a shipped heuristic, write a record of the same name at
`/params/heuristics/<name>`. That record replaces the shipped heuristic of
its name and leaves the other shipped one in place.

## The value record

A value is a `params/value/v1` record at `/params/values/<name>`, with the
same `<name>` as its heuristic.

```json
{
  "$type": "params/value/v1",
  "heuristic": "overflow-agent-grace",
  "value": 100,
  "scopes": {"/repos/board": 180},
  "data": "enough",
  "evidence": "/suites/params-overflow-agent-grace/runs/20261108T090000Z",
  "derived": "2026-11-08T09:00:00Z",
  "by": {"program": "claude-code"}
}
```

- `heuristic` is the name of the heuristic run, which is the value's own
  `<name>`. It is a name because a shipped heuristic has no record to refer
  to. `evidence` references the run of `/suites/params-<name>` that holds the
  inputs measured.
- `value` applies to any scope that `scopes` does not list. `scopes` maps
  the path of a repository or project to its own value.
- `data` is `enough` or `too-little`. With `too-little`, readers use the
  heuristic's default.
- `derived` is when the agent ran the heuristic, in UTC, and `by` names who
  ran it.

`board put` checks a value record only against its own JSON Schema. It writes
a value outside its heuristic's bounds, or one whose `heuristic` names another
heuristic. Such a record is unusable: `params.get` gives the default and
`params.due` lists it for an agent to derive again. One number outside the
bounds, in `value` or in `scopes`, makes the whole record unusable.

A check is a run of `/suites/params-<name>` whose `measures` hold the
measure as `check`.

## Capabilities

`board call params.get --name <name> [--scope <path>]` returns `name`,
`value` and `source`.

- With `source: value`, the number comes from the value record. It is the
  scope's value when `scopes` lists the scope, and the record's `value`
  otherwise. `record` and `derived` name the record.
- With `source: default`, the number is the heuristic's default, and
  `reason` says why. Every reason except `too-little` also gives a `warning`
  with a `code` and a `message`:

  | `reason` | Why the default applies | `warning.code` |
  | --- | --- | --- |
  | `missing` | No value record exists. | `params::value::missing` |
  | `too-little` | The last run found too little data. | none |
  | `out-of-bounds` | A number in the record is outside the bounds. | `params::value::out_of_bounds` |
  | `wrong-heuristic` | `heuristic` is not the value's own `<name>`. | `params::value::wrong_heuristic` |

- A name with no heuristic, shipped or stored, is refused with
  `params::heuristic::not_found`, because there is no default to give. The
  consumer then uses its own built-in default and warns.

A plugin built into Board calls `board_plugin_params::get` in-process for
the same answer.

`board call params.due` returns `due`, each value that needs an agent, by
`name` and `reason`, and `checked`, the number of heuristics read.

- `on-default`: the value has no record, or its last run found too little
  data. Board cannot read a heuristic's `enough`, so such a value is listed
  on every run until an agent finds enough data.
- `out-of-bounds` and `wrong-heuristic`: the value record is unusable, for the
  reason `params.get` gives.
- `check-failed`: the latest check run that started at or after `derived`
  measured `check` above the heuristic's `fails_above`. A check run from
  before the value was derived checked an earlier value, so it is not
  counted.
- `stale`: the value is older than its heuristic's `recheck_after` days.

A value is listed once, with the first of these that holds: unusable or on
the default, then `check-failed`, then `stale`.

Values without a record come first, then the rest by `derived`, oldest
first.

Both capabilities read only the board's own `/params/` and `/suites/`, not
its mounts. A record Board marks degraded for anything but a reference is
read as absent.

## Enabling

Board has the plugin built in. Enable `runs` first:

    board plugin enable runs
    board plugin enable params

`board plugin enable params` writes both schemas to `.board/types/params/`
and nothing else.

## Developing

The plugin is the crate `crates/board-plugin-params`. It generates both
record schemas from the structs in `src/records.rs`. Its `types/` directory
holds them as committed goldens, which a test compares with what the
structs generate. The tests through the binary are in
`crates/board-cli/src/tests/built_in/params.rs`.
