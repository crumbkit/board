"""The `plan` and `brief` capabilities: a change's plan, the records it uses, and what an agent needs to start."""

import json
import re
from datetime import UTC, datetime
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import FINAL, PLAN_TYPE, PLUGIN, TREE_CODES, read, write
from board_changes.phases import next_step, referenced

# The most words a plan's text holds, and the most words the documents it uses hold in all.
PLAN_WORDS = 3_000
USES_WORDS = 3_000
# The most words `brief` returns.
BRIEF_WORDS = 600
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
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    if change["phase"] in FINAL:
        raise PluginError(
            "changes::phase::final",
            f"the change at {path} is {change['phase']}, so it takes no new plan; "
            "list open changes with `board call changes.open`, or start a new one",
            {"path": path, "phase": change["phase"]},
        )
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
    stamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    record, written = f"{path}/plans/{stamp}", f"{path}/plans/{stamp}.md"
    change["plan"] = written
    write(
        board,
        [
            {"path": written, "expect": "none", "text": text},
            {"path": record, "expect": "none", "record": {"$type": PLAN_TYPE, "text": written, "uses": uses}},
            {"path": path, "expect": blob, "record": change},
        ],
    )
    return {"path": record, "text": written, "uses": len(uses)}


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


def newest(board: Board, path: str) -> tuple[str | None, list[str]]:
    """The text path and `uses` of the change's newest plan record, or no text and no uses."""
    prefix = f"{path}/plans/"
    rows = board.rows(
        f"SELECT body FROM records WHERE type = {literal(PLAN_TYPE)}"  # noqa: S608 - values are quoted by literal()
        f" AND substr(logical, 1, {len(prefix)}) = {literal(prefix)}"
        f" AND (code IS NULL OR code IN ({', '.join(literal(code) for code in TREE_CODES)}))"
        " ORDER BY logical DESC LIMIT 1"
    )
    if not rows:
        return None, []
    body = json.loads(rows[0][0])
    return str(body["text"]), [str(ref) for ref in body["uses"]]


def brief(request: CallRequest) -> dict[str, object]:
    """The newest plan, one line per record it uses within the word limit, the next step and the controls command."""
    path = str(request.input["path"])
    board = Board(request.board.root, PLUGIN)
    change, _ = read(board, path)
    text, uses = newest(board, path)
    records, _ = referenced(board, uses)
    texts = documents(board, uses, records)
    step = next_step(request)
    controls = f"board call controls.check --path {change['mount']}"
    spent = words(" ".join([path, text or "", str(step["phase"]), str(step["instruction"]), controls]))
    shown: list[dict[str, str]] = []
    for ref in uses:
        line = line_of(records.get(ref), texts.get(ref))
        cost = words(f"{ref} {line}")
        if spent + cost > BRIEF_WORDS:
            break
        shown.append({"ref": ref, "line": line})
        spent += cost
    return {
        "path": path,
        "plan": text,
        "uses": shown,
        "left_out": len(uses) - len(shown),
        "phase": step["phase"],
        "instruction": step["instruction"],
        "controls": controls,
    }
