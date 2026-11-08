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
- `bounds` holds `min` and `max`. A value outside them is wrong.
- `check` holds a `measure`, with the command that computes it, and
  `fails_above`. When the measure is greater than `fails_above`, the value
  is wrong.
- `recheck_after` is the number of active days after which the value is
  due again. `consumer` names the plugin that reads the value.

The plugin ships two heuristics, `overflow-agent-grace` and
`overflow-owner-grace`, in `crates/board-plugin-params/heuristics/` in
Board's repository. Each scales a starting grace of changed lines by how
busy a repository is against the median of the board's repositories, and
writes a value per repository under `scopes`.

## The value record

A value is a `params/value/v1` record at `/params/values/<name>`, with the
same `<name>` as its heuristic.

```json
{
  "$type": "params/value/v1",
  "heuristic": "/params/heuristics/overflow-agent-grace",
  "value": 100,
  "scopes": {"/repos/board": 180},
  "data": "enough",
  "evidence": "/suites/params-overflow-agent-grace/runs/20261108T090000Z",
  "derived": "2026-11-08T09:00:00Z",
  "by": {"program": "claude-code"}
}
```

- `heuristic` references the heuristic run, and `evidence` the run of
  `/suites/params-<name>` that holds the inputs measured.
- `value` applies to any scope that `scopes` does not list. `scopes` maps
  the path of a repository or project to its own value.
- `data` is `enough` or `too-little`. With `too-little`, readers use the
  heuristic's default.
- `derived` is when the agent ran the heuristic, in UTC, and `by` names who
  ran it.

A check is a run of `/suites/params-<name>` whose `measures` hold the
measure as `check`.

## Capabilities

`board call params.get --name <name> [--scope <path>]` returns `name`,
`value` and `source`.

- With `source: value`, the number comes from the value record. It is the
  scope's value when `scopes` lists the scope, and the record's `value`
  otherwise. `record` and `derived` name the record.
- With `source: default`, the number is the heuristic's default, and
  `reason` says why: `missing` when there is no value record, which is the
  warning `params::value::missing`, or `too-little`.
- A name with no heuristic is refused with `params::heuristic::not_found`.
  The consumer then uses its own built-in default and warns.

A plugin built into Board calls `board_plugin_params::get` in-process for
the same answer.

`board call params.due` returns `due`, each value that needs an agent, by
`name` and `reason`, and `checked`, the number of heuristics read.

- `on-default`: the value has no record, or its last run found too little
  data.
- `check-failed`: the latest check run that started at or after `derived`
  measured `check` above the heuristic's `fails_above`. A check run from
  before the value was derived checked an earlier value, so it is not
  counted.

Values without a record come first, then the rest by `derived`, oldest
first.

Both capabilities read only the board's own `/params/` and `/suites/`, not
its mounts. A record Board marks degraded for anything but a reference is
read as absent.

## Enabling

Board has the plugin built in. Enable `runs` first:

    board plugin enable runs
    board plugin enable params

`board plugin enable params` writes both schemas to `.board/types/params/`.

## Developing

The plugin is the crate `crates/board-plugin-params`. It generates both
record schemas from the structs in `src/records.rs`. Its `types/` directory
holds them as committed goldens, which a test compares with what the
structs generate. The tests through the binary are in
`crates/board-cli/src/tests/built_in/params.rs`.
