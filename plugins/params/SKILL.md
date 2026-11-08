---
name: params
description: 'Calibrated values, such as overflow''s graces, each with the heuristic that derives it and the run that set it. Use it to read a value a plugin uses, and when a scheduled run asks you to recalibrate. Read one with `board call params.get --name <name>`, which gives the heuristic''s default and why when no value applies. `board call params.due` lists the values to derive again. SKILL.md and README.md in the plugin''s directory say more.'
---

# params

A calibrated value is a number a plugin uses, such as how many changed
lines a control may run over before an agent drafts a step. Each value has a
heuristic, either shipped with Board or at `/params/heuristics/<name>`, and,
once an agent has run it, a value record at `/params/values/<name>`. A record
at `/params/heuristics/<name>` overrides the shipped heuristic of that name.
Board stores and resolves the values. An agent runs each heuristic's method.

## Read a value

Run `board call params.get --name <name>`, adding `--scope /repos/<repo>`
when the value is for one repository or project. The output's `value` is
the number to use.

- When `source` is `value`, cite `record` and `derived` in any output that
  used the number.
- When `source` is `default`, the heuristic's lenient default applies.
  `reason` says why: `missing` when no value record exists, `too-little`
  when the last run found too little data, `out-of-bounds` when a number in
  the record is outside the heuristic's bounds, and `wrong-heuristic` when
  the record names another heuristic. Except for `too-little`, `warning`
  gives a code and a message to pass on.
- A refusal with `params::heuristic::not_found` means there is no heuristic
  of that name. Use your own built-in default and warn.

Never copy a value into another record. Read it again each time.

## Recalibrate

A scheduled run in the personal board asks an agent to recalibrate. Follow
these steps there.

1. For each heuristic, run its `check`: compute the measure its `measure`
   describes. Record a `runs/run/v1` of the suite `/suites/params-<name>`
   that gives the result in the run's `measures` as `check`. Create the
   suite on first use, with the heuristic's `method` as its method.
2. Run `board call params.due`. It lists each value due, oldest first, with
   a reason: `check-failed` when the latest check since the value was
   derived measured above the check's `fails_above`, `stale` when the value
   is older than the heuristic's `recheck_after` calendar days, `on-default`
   when the value has no record or its last run found too little data, and
   `out-of-bounds` or `wrong-heuristic` when readers cannot use the record.
3. For each value listed, read its heuristic. Measure each input with the
   command its `source` gives, and record the inputs in a run of
   `/suites/params-<name>`. Decide from `enough` whether there is enough
   data.
4. Write the value record with `board put /params/values/<name>`. Give
   `heuristic` as `/params/heuristics/<name>`, the `value` the method
   derives, `scopes` for each repository or project with its own value,
   `data` as `enough` or `too-little`, `evidence` as the run, `derived` as
   the current UTC time and `by` as yourself. Keep every number within the
   heuristic's `bounds`, because `board put` does not check them and readers
   use the default for a record with a number outside them.

When there is too little data, still write the record with `data:
too-little`, so the next run knows when the data was last looked at. Git
keeps the earlier values.

Do not edit a heuristic unless the owner asks. The owner sets its start,
default, bounds and check.
