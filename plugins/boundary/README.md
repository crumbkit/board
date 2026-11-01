# boundary

A Board plugin that checks content before it leaves a checkout. A boundary
names the controls that content must pass to go to a destination by an
action. `boundary.check` evaluates the content through
`controls.evaluate` and records the verdicts as a crossing. The plugin knows
no destination and sends nothing; the action that sends content calls it
first and sends only on a pass.

Boundaries, the controls they name and crossings all live in the personal
board, the one `personal` names in `$XDG_CONFIG_HOME/board/config.json`. Each
capability reads and writes there through `board --personal`, whichever board
calls it. Enable `controls` and `boundary` in the personal board, and
`boundary` in any board that calls it.

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

- `to`, `actions` and `checks` are required. Boundaries live under
  `/boundaries/` in the personal board, such as `/boundaries/octo`.
- `to` is a glob of destinations, and `actions` holds globs of action
  names, such as `github.pr-create`. Globs are shell-style and match case:
  `*` matches any characters, `/` and `.` included.
- `from` holds references to checkouts, usually mounts. It matches a
  checkout at or under one of its paths, compared as text, so a checkout
  named through a link is not matched. Without `from`, the boundary covers
  any checkout, including one no board mounts.
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
free one. The groups run one after another within the capability's 1,800
seconds. A refusal from `controls.evaluate`, such as
`controls::control::not_found`, is `boundary::board::refused` carrying that
code. Any entry under `/boundaries/` that is not a valid boundary, because
it does not parse, has another type or fails its schema, refuses every
check with `boundary::boundary::invalid`, naming each entry and its code, so
a broken rule stops content. The check passes `BOARD_CONTROLS_AGENT`,
`ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` on, for judged rules.

Then it writes the crossing and returns `digest`, `crossing`, `boundaries`,
`verdicts` and `note`. `note` says why nothing was checked when no boundary
matched or no check named a part; otherwise it is null. When any verdict
failed, the check refuses with `boundary::check::failed`. Its message names
each failing control, part and line with the quoted text and any rewrite,
the first 20 of them, and ends with the next step. Its fields hold `digest`,
`crossing`, every failure in `failures` (`control`, `part`, `text`, `line`
and `rewrite` when known), and `recovery`, `{kind: hint, text}`, whose text
starts `Next:`. It says what to change, including giving a control
without a rule a rule, and names `board call <action>` and
`board --personal call boundary.check` to run again. A pattern control shown `match-only` never reveals its
expression here either.

`boundary.verify` takes `--digest`, or `action`, `to` and `parts` on stdin,
and returns `digest`, `crossing` (null when there is none), `passed` and
`result`. Giving both or a part of the content is
`boundary::verify::bad_input`.

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
`controls.evaluate` gave it, and `result` once recorded. It holds no part's
text, but it keeps each finding's quoted text. `from`, `boundaries` and
each verdict's `control` are references, so deleting a boundary or control
leaves an old crossing degraded but still read. Checking the same content
again replaces the crossing; a re-check that passes keeps its `result`, and
one that fails drops it.

## Developing

Install the plugin by linking its directory in this repository. The gates,
which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by
default the workspace's `target/debug/board`, against a temporary personal
board named in a temporary `XDG_CONFIG_HOME`, and start a stub agent script,
never a model. `plugin.json` and the shipped schemas are generated from
`src/board_boundary/shapes.py` by `board_boundary.manifest`, and a test
checks that the committed files match.
