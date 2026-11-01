# boundary

A Board plugin, built into the `board` binary, that checks content before it
leaves a checkout. A boundary names the controls that content must pass to
go to a destination by an action. `boundary.check` evaluates the content
through `controls.evaluate` and records the verdicts as a crossing. The
plugin knows no destination and sends nothing; the action that sends
content calls it first and sends only on a pass.

Boundaries, the controls they name and crossings all live in the personal
board, the one `personal` names in `$XDG_CONFIG_HOME/board/config.json`.
Each capability reads, calls, asks and writes there, whichever board calls
it. Enable `controls` and `boundary` in the personal board, and `boundary`
in any board that calls it.

## The boundary record

```json
{
  "$type": "boundary/boundary/v1",
  "from": ["/repos/board"],
  "to": "github:octo/*",
  "actions": ["github.*"],
  "checks": [
    {"control": "/controls/plain-prose", "parts": ["pr.*", "comment.body"]},
    {"control": "/controls/private-names", "parts": ["*"]}
  ]
}
```

- `to`, `actions` and `checks` are required. Boundaries belong under
  `/boundaries/` in the personal board, such as `/boundaries/octo`, and
  `/boundaries/` must be a directory. A valid boundary elsewhere is still
  read. A file git ignores is not read.
- `to` is a glob of destinations, and `actions` holds globs of action
  names, such as `github.pr-create`. Globs are SQLite GLOB patterns, as
  controls' `paths` are: `*` matches any characters, `/` and `.` included,
  `?` one character, and `[...]` one of a set, `[^...]` its complement.
  Destinations match without case, as GitHub names do; action and part
  names match with case.
- `from` holds references to checkouts, usually mounts. It matches a
  checkout at or under one of its paths, compared as text, so a checkout
  named through a link is not matched. Without `from`, the boundary covers
  any checkout, including one no board mounts. A check that names no
  checkout is covered by every boundary, `from` or not.
- Each check names a control in the personal board and globs of the part
  names it applies to. Board checks the reference, so a deleted control
  makes the boundary `board::ref::dangling`. Each control must have a rule.
  A control without one cannot check content, so its verdict, `stated`,
  fails the check.
- A part whose name no check's globs match is not checked. Write `*` to
  check every part.

## Capabilities

`boundary.check` takes `action`, `to`, optional `from` and `parts`, a map
from part name to text, on stdin. It finds every boundary that matches, and
gathers for each part the controls whose globs name it. Parts with the same
controls go to one `controls.evaluate`, so a part's budget and pattern rules
run before its judged rules, and a judged rule skips a part that failed a
free one. A refusal from `controls.evaluate`, such as
`controls::control::not_found`, is `boundary::board::refused` carrying that
code. Until Board can ask a model, a judged rule with a part to read is
refused that way with `controls::judged::unavailable`.
Results that the plugin cannot read, which happens only when the boundary
and controls plugins come from different Board builds, refuse the check
with `boundary::evaluate::unreadable`.

Any entry under `/boundaries/` that is not a valid boundary, because it does
not parse, has another type or fails its schema, a boundary elsewhere that
fails its schema, and a `/boundaries` that is a symlink or a file each
refuse every check with `boundary::boundary::invalid`. Its `boundaries`
field maps each entry to its code, `not a boundary` or `is not a directory`,
and the hint says to fix the record, move the entry out of `/boundaries/`,
or make `/boundaries/` a directory. The capability declares
`BOARD_CONTROLS_AGENT`, `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN`,
which a judged rule reads, and a timeout of 1,800 seconds.

Then it writes the crossing and returns `digest`, `crossing`, `boundaries`,
`verdicts`, `unchecked`, the parts no check named, `note` and `override`.
`note` says why nothing was checked when no boundary matched or no check
named a part; otherwise it is null. When any verdict failed, the check
refuses with `boundary::check::failed`. Its message names each failing
control, part and line with the quoted text and any rewrite, the first 20
of them. Its fields hold `digest`, `crossing` and every failure in
`failures` (`control`, `part`, `text`, and `line` and `rewrite` when
known). Its hint starts `Next:`, says what to change, including giving a
control without a rule a rule, and names `board call <action>` and
`board --personal call boundary.check` to run again. A pattern control
shown `match-only` never reveals its expression here either.

## Overrides

The owner can let one exact content through a failed check without
changing a rule. Only a failed check asks; a passing check writes no effect.

- A failed check asks Board to decide the op `boundary.override` at
  `/crossings/<digest>` in the personal board, as plugin `boundary`, showing
  the owner the action, destination and each part. Board writes a pending
  effect in the personal board, and the refusal's `effect` field holds its
  id. The hint ends saying the owner may approve the content with
  `board answer <effect> approve`, which Board signs with Touch ID.
  Checking the same content again asks nothing new.
- Once the owner approves, the same check passes. The crossing records
  `passed: true`, every verdict as it was, and `override`, the approved
  effect's id. The output's `override` holds the same id; it is null for a
  check that passed on its own. `boundary.verify` reports the crossing as
  passed, and `boundary.record` accepts it.
- The digest covers the action, destination and every part, so an approval
  admits exactly that content. Any other content has another crossing path
  and asks again.
- The text quotes the action, destination and each part name as JSON, and
  indents every line of a part by two spaces, so no two contents show the
  same text.
- When the owner refused the effect, the check still fails. The hint says
  the owner refused this content, names the effect, and says the owner can
  ask again by deleting `/effects/seen/<effect>`.
- Content over 1 MiB as shown cannot be asked. Board refuses the decide
  with `board::json::too_large`, and the check fails as usual, with no
  `effect`, its hint ending that the owner cannot approve content this
  large.
- The text shows the content, not the failed controls, so an approval keeps
  covering the content if a later control also fails it.
- An approval does not expire and covers one crossing. The owner withdraws
  one by deleting its seen record from `/effects/seen/`.

`boundary.verify` takes `--digest`, or `action`, `to` and `parts` on stdin,
and returns `digest`, `crossing` (null when there is none), `passed` and
`result`. `passed` is true only while the boundaries that match the
crossing's action, destination and checkout are the ones it was checked
against. It does not notice an edit to a matched boundary's checks or to a
control's rule, so a gate that must see those runs `check` again. Giving
both, or a part of the content, is `boundary::verify::bad_input`.

`boundary.record --digest <digest> --result <text>` sets `result` on a
passing crossing and returns `crossing` and `result`. A crossing that does
not exist is `boundary::crossing::not_found`, and one that failed is
`boundary::crossing::failed`.

## The crossing record

`boundary/crossing/v1` lives at `/crossings/<digest>`. The digest is the
SHA-256, in lowercase hex, of the UTF-8 JSON
`{"action": ..., "parts": ..., "to": ...}` with keys sorted, no whitespace
and characters unescaped, as Python's
`json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`
writes it. The record holds the `digest`, `action`, `to`, `from` when
given, `passed`, the matching `boundaries`, each verdict in `verdicts` as
`controls.evaluate` gave it with its nulls left out, `override` when the
owner's approval let it through, and `result` once recorded. It holds no
part's text, but it keeps each finding's quoted text. `from`, `boundaries`
and each verdict's `control` are references, so deleting a boundary or
control leaves an old crossing degraded but still read. Checking the same
content again replaces the crossing; a re-check that passes keeps its
`result`, and one that fails drops it.

## Developing

The plugin is the crate `crates/board-plugin-boundary`. It generates the
manifest's input and output schemas and both record schemas from Rust
structs with schemars. Its `types/` directory holds the record schemas as
committed goldens, which a test compares with what the structs generate;
`SNAPSHOTS=overwrite` rewrites them. This directory keeps `SKILL.md`. The
tests of the plugin through the binary are in
`crates/board-cli/src/tests/built_in/boundary.rs` and the files beside it in
`boundary/`. They use a personal board named in a temporary known boards
file and approve an override with `board answer` and the test signer.
