"""The `plan` and `brief` capabilities: a change's plan, the records it uses, and what an agent needs to start."""

import json
import re
from dataclasses import replace
from datetime import UTC, datetime
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import FINAL, PLAN_TYPE, PLUGIN, kind_of, newest, read, write
from board_changes.phases import next_step, referenced

# The most words a plan's text holds, and the most words the documents it uses hold in all.
PLAN_WORDS = 3_000
USES_WORDS = 3_000
# The most words `brief` returns.
BRIEF_WORDS = 600
# The most planned paths a rule lists as matched; it counts the rest, so a rule costs about one line.
MATCHED_SHOWN = 3
# The codes Board refuses a call with when the controls plugin or its capability is missing.
MISSING = ("board::plugin::not_installed", "board::plugin::not_enabled")
CONTROL_TYPE = "controls/control/v1"
DECISION_TYPE = "plans/decision/v1"
EVAL_TYPE = "evals/eval/v1"
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")


def words(text: str) -> int:
    """How many whitespace-separated words `text` holds."""
    return len(text.split())


def documents(board: Board, refs: list[str], records: dict[str, tuple[str, dict[str, Any]]]) -> dict[str, str]:
    """The text of each Markdown document among `refs`, by reference."""
    texts = [ref for ref in refs if ref.endswith(".md") and records.get(ref, ("",))[0] == "file"]
    return {
        ref: found.decode(errors="replace")
        for ref, found in zip(texts, board.get(texts), strict=True)
        if not isinstance(found, PluginError)
    }


def plan(request: CallRequest) -> dict[str, object]:
    """Write a plan record and its text under the change, and point the change's `plan` at the text, in one write."""
    asked = request.input
    path, text = str(asked["path"]), str(asked["text"])
    # Board validated the input, so `uses` is a list of paths; each is kept once, in order.
    uses = list(dict.fromkeys(cast("list[str]", asked["uses"])))
    paths = list(dict.fromkeys(cast("list[str]", asked.get("paths", []))))
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    if change["phase"] in FINAL:
        raise PluginError(
            "changes::phase::final",
            f"the change at {path} is {change['phase']}, so it takes no new plan; "
            "list open changes with `board call changes.open`, or start a new one",
            {"path": path, "phase": change["phase"]},
        )
    check_paths(path, change["mount"], paths, kind_of(change))
    if words(text) > PLAN_WORDS:
        raise PluginError(
            "changes::plan::too_long",
            f"the plan for {path} holds {words(text):,} words, over the {PLAN_WORDS:,}-word limit; "
            "cut it, and move detail into the records it uses",
            {"path": path, "words": words(text)},
        )
    records, _ = referenced(board, uses)
    dangling = [ref for ref in uses if ref not in records]
    if dangling:
        raise PluginError(
            "changes::ref::dangling",
            f"these references in the plan reach nothing or are degraded: {', '.join(dangling)}; "
            "check each path with `board ls` or `board check`",
            {"path": path, "refs": dangling},
        )
    directories = [ref for ref in uses if records[ref][0] == "directory"]
    if directories:
        raise PluginError(
            "changes::plan::directory",
            f"the plan uses the directories {', '.join(directories)}, whose words the limit cannot count; "
            "use the documents in them that apply, choosing from each directory's index.md",
            {"path": path, "refs": directories},
        )
    used = sum(words(found) for found in documents(board, uses, records).values())
    if used > USES_WORDS:
        raise PluginError(
            "changes::plan::uses_too_long",
            f"the documents the plan for {path} uses hold {used:,} words in all, over the {USES_WORDS:,}-word "
            "limit; use fewer, choosing from each directory's index.md",
            {"path": path, "words": used},
        )
    record, lines = plan_lines(path, text, uses, paths)
    change["plan"] = lines[0]["path"]
    write(board, [*lines, {"path": path, "expect": blob, "record": change}])
    return {"path": record, "text": change["plan"], "uses": len(uses)}


def check_paths(path: str, mount: str, paths: list[str], kind: str) -> None:
    """Refuse a build change's plan that declares no paths, and any path outside the change's mount."""
    if not paths and kind == "build":
        raise PluginError(
            "changes::plan::no_paths",
            f"the plan for {path} declares no paths; give `paths`, the files and SQLite GLOB patterns it will "
            f"change by mounted path, such as {mount}/src/**, so its brief can list their rules",
            {"path": path},
        )
    outside = [planned for planned in paths if not planned.startswith(mount.rstrip("/") + "/")]
    if outside:
        raise PluginError(
            "changes::plan::outside_mount",
            f"these planned paths are outside the change's repository {mount}: {', '.join(outside)}; "
            f"start each with {mount}/",
            {"path": path, "paths": outside},
        )


def plan_lines(path: str, text: str, uses: list[str], paths: list[str]) -> tuple[str, list[dict[str, Any]]]:
    """A new plan record for the change at `path`, and the `put` lines that write its text and then the record."""
    stamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    record, written = f"{path}/plans/{stamp}", f"{path}/plans/{stamp}.md"
    body: dict[str, Any] = {"$type": PLAN_TYPE, "text": written, "uses": uses}
    if paths:
        body["paths"] = paths
    return record, [
        {"path": written, "expect": "none", "text": text},
        {"path": record, "expect": "none", "record": body},
    ]


def line_of(found: tuple[str, dict[str, Any]] | None, text: str | None) -> str:
    """One line saying what a used record or document holds; `text` is a document's text."""
    if found is None:
        return "unreadable: reaches nothing or is degraded"
    type_name, body = found
    if type_name == CONTROL_TYPE:
        return str(body.get("summary") or body.get("requirement", ""))
    if type_name == DECISION_TYPE:
        return str(body.get("decision", ""))
    if type_name == EVAL_TYPE:
        verdict = body.get("verdict")
        return f"{body.get('question', '')} Verdict: {verdict}." if verdict else str(body.get("question", ""))
    if text is not None:
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        headings = [match.group(1) for line in lines if (match := HEADING.match(line))]
        return next(iter(headings or lines), "")
    return type_name


def rules_for(board: Board, paths: list[str]) -> list[dict[str, Any]]:
    """Each control that applies to `paths`, once, as `controls.for-paths` answers; none for no paths.

    A Board without the controls plugin enabled, or one too old to offer
    `for-paths`, is `changes::brief::controls_missing`.
    """
    if not paths:
        return []
    try:
        report = board.document("call", "controls.for-paths", stdin=for_paths_input(paths).encode())
    except PluginError as error:
        if error.code == f"{PLUGIN}::board::refused" and (error.fields or {}).get("code") in MISSING:
            why = (
                f"the brief lists the rules on the planned paths with controls.for-paths, which this Board "
                f"refused: {error.message}; enable it with `board plugin enable controls`, or run a Board that "
                "offers it"
            )
            raise PluginError(f"{PLUGIN}::brief::controls_missing", why) from error
        raise
    return cast("list[dict[str, Any]]", report["data"]["output"]["controls"])


def for_paths_input(paths: list[str]) -> str:
    """The input `controls.for-paths` takes for `paths`, as JSON."""
    return json.dumps({"paths": paths})


def rule_line(rule: dict[str, Any], paths: list[str]) -> dict[str, Any]:
    """`rule` as the brief lists it: `matched` left out when it is every planned path, and cut to MATCHED_SHOWN."""
    line: dict[str, Any] = {"control": rule["control"], "summary": rule["summary"]}
    matched: list[str] = rule["matched"]
    if set(matched) == set(paths):
        return line
    line["matched"] = matched[:MATCHED_SHOWN]
    if len(matched) > MATCHED_SHOWN:
        line["matched_left_out"] = len(matched) - MATCHED_SHOWN
    return line


def brief(request: CallRequest) -> dict[str, object]:
    """The newest plan, its paths' rules and one line per record it uses within the word limit, and the next step."""
    return brief_of(request, str(request.input["path"]))


def brief_of(request: CallRequest, path: str) -> dict[str, object]:
    """The brief of the change at `path`, read from the board `request` names."""
    board = Board(request.board.root, PLUGIN)
    change, _ = read(board, path)
    text, uses, paths = newest(board, path)
    records, _ = referenced(board, uses)
    texts = documents(board, uses, records)
    step = next_step(replace(request, input={"path": path}))
    controls = f"board call controls.check --path {change['mount']}"
    brief_line = f"board-brief: {path}"
    spent = words(" ".join([brief_line, path, text or "", str(step["phase"]), str(step["instruction"]), controls]))
    rules_command = f"board call controls.for-paths <<< '{for_paths_input(paths)}'" if paths else None
    spent += words(rules_command or "")
    found = [rule_line(rule, paths) for rule in rules_for(board, paths)]
    rules: list[dict[str, Any]] = []
    for at, rule in enumerate(found):
        cost = words(" ".join([rule["control"], rule["summary"], *rule.get("matched", [])]))
        # Each rule after this one costs one word in `rules_left_out` if it does not fit.
        if spent + cost + len(found) - at - 1 > BRIEF_WORDS:
            break
        rules.append(rule)
        spent += cost
    left_out = [str(rule["control"]) for rule in found[len(rules) :]]
    spent += len(left_out)
    shown: list[dict[str, str]] = []
    for ref in uses:
        line = line_of(records.get(ref), texts.get(ref))
        cost = words(f"{ref} {line}")
        if spent + cost > BRIEF_WORDS:
            break
        shown.append({"ref": ref, "line": line})
        spent += cost
    return {
        "brief_line": brief_line,
        "path": path,
        "plan": text,
        "rules": rules,
        "rules_left_out": left_out,
        "rules_command": rules_command,
        "uses": shown,
        "left_out": len(uses) - len(shown),
        "phase": step["phase"],
        "instruction": step["instruction"],
        "controls": controls,
    }
