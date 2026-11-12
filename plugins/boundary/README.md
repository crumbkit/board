# boundary

A Board plugin, built into the `board` binary, that checks content before it
leaves a checkout. A boundary names the rules that content must pass to
go to a destination by an action. `boundary.check` evaluates the content
through `rules.evaluate` and records the verdicts as a crossing. The
plugin knows no destination and sends nothing; the action that sends
content calls it first and sends only on a pass.

Boundaries, the rules they name and crossings all live in the personal
board, the one `personal` names in `$XDG_CONFIG_HOME/board/config.json`.
Each capability reads, calls, asks and writes there, whichever board calls
it. Enable `rules` and `boundary` in the personal board, and `boundary`
in any board that calls it.

## The boundary record

```json
{
  "$type": "boundary/boundary/v1",
  "from": ["/repos/board"],
  "to": "github:octo/*",
  "actions": ["github.*"],
  "checks": [
    {"rule": "/rules/plain-prose", "parts": ["pr.*", "comment.body"]},
    {"rule": "/rules/private-names", "parts": ["*"]}
  ]
}
```

- `to`, `actions` and `checks` are required. Boundaries belong under
  `/boundaries/` in the personal board, such as `/boundaries/octo`, and
  `/boundaries/` must be a directory. A valid boundary elsewhere is still
  read. A file git ignores is not read.
- `to` is a glob of destinations, and `actions` holds globs of action
  names, such as `github.pr-create`. Globs are SQLite GLOB patterns, as
  rules' `paths` are: `*` matches any characters, `/` and `.` included,
  `?` one character, and `[...]` one of a set, `[^...]` its complement.
  Destinations match without case, as GitHub names do; action and part
  names match with case.
- `from` holds references to checkouts, usually mounts. It matches a
  checkout at or under one of its paths, compared as text, so a checkout
  named through a link is not matched. Without `from`, the boundary covers
  any checkout, including one no board mounts. A check that names no
  checkout is covered by every boundary, `from` or not.
- Each check names a rule in the personal board, in `rule`, and globs of
  the part names it applies to. A boundary written before the rename names
  the rule in `control`, which reads the same. Board checks the reference,
  so a deleted rule makes the boundary `board::ref::dangling`. Each rule
  must have a checker. A rule without one cannot check content, so its
  verdict, `stated`, fails the check.
- A part whose name no check's globs match is not checked. Write `*` to
  check every part.

## Capabilities

`boundary.check` takes `action`, `to`, optional `from` and `parts`, a map
from part name to text, on stdin. It finds every boundary that matches, and
gathers for each part the rules whose globs name it. Parts with the same
rules go to one `rules.evaluate`, so a part's budget and pattern checkers
run before its judged ones, and a judged checker skips a part that failed a
free one. A refusal from `rules.evaluate`, such as
`rules::rule::not_found`, is `boundary::board::refused` carrying that
code. A judged rule with a part to read is refused that way with
`rules::agent::not_set` when `BOARD_CONTROLS_AGENT` names no agent.
Results that the plugin cannot read refuse the check with
`boundary::evaluate::unreadable`. That happens when the boundary and
rules plugins come from different Board builds.

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
rule, part and line with the quoted text and any rewrite, the first 20
of them. Its fields hold `digest`, `crossing` and every failure in
`failures` (`rule`, `part`, `text`, and `line` and `rewrite` when
known). Its hint starts `Next:`, says what to change, including giving a
rule without a checker a checker, and names `board call <action>` and
`board --personal boundary check` to run again. A pattern rule whose
`quote` is `match-only` never reveals its expression here either.

`boundary.covers --action <action> --to <destination> [--from <checkout>]`
returns `boundaries`, the sorted paths of the boundaries a check of that
action, destination and checkout would apply, by the same rules. It checks
and writes nothing. It refuses with `boundary::boundary::invalid` as `check`
does. The github plugin's Claude Code mod asks it, through `github.covers`,
before a raw `git push` or `gh` write.

## Overrides

The owner can let one exact content through a failed check without
changing a rule. Only a failed check asks; a passing check writes no effect.

- A failed check asks Board to decide the capability `boundary.override` at
  `/crossings/<hex>` in the personal board, as plugin `boundary`, showing
  the owner the action, destination and each part. Board writes a pending
  effect in the personal board, and the refusal's `effect` field holds its
  id. The hint ends saying the owner may approve the content with
  `board answer <effect> approve`, which Board signs with Touch ID.
  Checking the same content again asks nothing new.
- Once the owner approves, the same check passes. The crossing records
  every verdict as it was, and `override`, the approved effect's id. The output's `override` holds the same id; it is null for a
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
- The text shows the content, not the failed rules, so an approval keeps
  covering the content if a later rule also fails it.
- An approval does not expire and covers one crossing. The owner withdraws
  one by deleting its seen record from `/effects/seen/`.

`boundary.verify` takes `--digest`, or `action`, `to` and `parts` on stdin,
and returns `digest`, `crossing` (null when there is none), `passed`,
`result` and `boundaries`, the paths the crossing was checked against.
`passed` is true only while the boundaries that match the crossing's
action, destination and checkout are the ones it was checked
against. It does not notice an edit to a matched boundary's checks or to a
rule's checker, so a gate that must see those runs `check` again. Giving
both, or a part of the content, is `boundary::verify::bad_input`.

`boundary.record --digest <digest> --result <text>` sets `result` on a
passing crossing and returns `crossing` and `result`. A crossing that does
not exist is `boundary::crossing::not_found`, and one that failed is
`boundary::crossing::failed`.

## The crossing record

`boundary/crossing/v1` lives at `/crossings/<hex>`. Its digest is
`sha256:` and `<hex>`, the SHA-256 in lowercase hex of the UTF-8 JSON
`{"action": ..., "parts": ..., "to": ...}` with keys sorted, no whitespace
and characters unescaped, as Python's
`json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`
writes it. The record holds the `digest`, `action`, `to`, `from` when
given, the matching `boundaries`, each verdict in `verdicts` as
`rules.evaluate` gave it, renamed, with its nulls left out, `override` when the
owner's approval let it through, and `result` once recorded. A verdict's
`rule` is the rule's path, and its `checker` is `budget`, `pattern`,
`judged` or `version`, as the rule's checker names it in `check`. A
crossing written before the rename names the path `control` and the
checker `rule`, and reads the same. The record stores no `passed`: `verify` and `record`
derive it. A crossing passes when no verdict failed, or when it holds an
`override`. A verdict passes when its status is `passed` or `skipped`. A
verdict whose status is `failed`, `stated` or one this Board does not know
fails. The crossing holds no part's text, but it keeps each finding's
quoted text. `from`, `boundaries` and an old verdict's `control` are
references, and a verdict's `rule` is a plain string until the former
names are dropped, because it holds a checker in an old crossing. Deleting
a boundary or rule leaves an old crossing
degraded but still read. Checking the same content again replaces the
crossing; a re-check that passes keeps its `result`, and one that fails
drops it.

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
