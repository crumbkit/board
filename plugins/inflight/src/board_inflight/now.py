"""The `now` capability: what is open or running, read from other plugins' records and capabilities.

It writes nothing. Changes and their next steps come from `changes.open` and
`changes.next`, questions from `plans.open` and evals from `evals.open`, all
through `board call`, so their rules live in their own plugins. Agents,
pull requests and artifacts are read from their records with `board query`.
"""

import datetime as dt
import json
import shlex
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

PLUGIN = "inflight"
AGENT_TYPE = "runs/agent/v1"
RUN_TYPE = "runs/run/v1"
OPENED_TYPE = "changes/opened/v1"
ARTIFACT_TYPE = "artifacts/artifact/v1"
VERSION_TYPE = "artifacts/version/v1"
# Codes that depend on the tree rather than the record. A record holding one is still a record to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")
READABLE = f"(code IS NULL OR code IN ({', '.join(literal(code) for code in TREE_CODES)}))"
# The most open changes `changes.open` gives, so every open change is known even when few are listed.
EVERY_CHANGE = 10_000
# Where a version record sits under its artifact's path.
VERSIONS = "/versions/"

type Entry = dict[str, Any]


def now(request: CallRequest) -> dict[str, object]:
    """The open changes with their steps, agents, pull requests and artifacts; other agents; questions; evals."""
    asked = request.input
    limit = int(cast("int", asked["limit"])) if "limit" in asked else 20
    holder = cast("str | None", asked.get("holder"))
    stopped = _instant(asked["stopped-at"]) if "stopped-at" in asked else None
    board = Board(request.board.root, PLUGIN)
    given = {"limit": EVERY_CHANGE} | ({"holder": holder} if holder is not None else {})
    opened = _call(board, "changes.open", given)
    every = cast("list[Entry]", opened["changes"])
    listed = every[:limit]
    agents, unreadable = _agents(board, stopped)
    seen = Seen(agents, _pull_requests(board), _artifacts(board))
    changes = [_change(board, change, seen, limit) for change in listed]
    open_paths = {str(change["path"]) for change in every}
    others = [
        agent
        for agent in agents
        if agent["works_on"] not in open_paths and (holder is None or agent["session"] == holder)
    ]
    return {
        "changes": {
            "changes": changes,
            "total": opened["total"],
            "left_out": int(opened["total"]) - len(listed),
            "unreadable": opened["unreadable"],
        },
        "unattached": {
            "agents": others[:limit],
            "total": len(others),
            "left_out": max(len(others) - limit, 0),
            "unreadable": unreadable,
        },
        "questions": _waiting(board, "plans.open", "questions", limit),
        "evals": _waiting(board, "evals.open", "evals", limit),
    }


def _call(board: Board, capability: str, given: Mapping[str, object]) -> Entry:
    """The output of `board call <capability>` with `given` as its flags.

    Board reads no stdin for a capability whose input is all flags, so each value goes as its flag.
    """
    arguments = [part for name, value in given.items() for part in (f"--{name}", str(value))]
    report = board.document("call", capability, *arguments)
    return cast("Entry", report["data"]["output"])


def _refusal(error: PluginError) -> dict[str, str]:
    """The code and message of the call that refused: Board's code when Board or the called plugin refused."""
    return {"code": str(error.fields.get("code", error.code)), "message": error.message}


@dataclass(frozen=True)
class Seen:
    """What the records say about every change: agents with no run, pull requests and artifacts, by change."""

    agents: list[Entry]
    pulls: dict[str, str]
    artifacts: dict[str, list[Entry]]


def _change(board: Board, change: Entry, seen: Seen, limit: int) -> Entry:
    """One open change with its next step and actor, its pull request, its agents and its artifacts."""
    path = str(change["path"])
    try:
        step = _call(board, "changes.next", {"path": path})
        nxt, actor, refused = step["next"], step["actor"], None
    except PluginError as error:
        nxt, actor, refused = None, None, _refusal(error)
    mine = [agent for agent in seen.agents if agent["works_on"] == path]
    held = seen.artifacts.get(path, [])
    url = seen.pulls.get(path)
    return {
        **{key: change[key] for key in ("path", "title", "repo", "branch", "holder", "worktree")},
        "next": nxt,
        "actor": actor,
        "refused": refused,
        "pull_request": url,
        "pr_state": None if url is None else f"gh pr view {shlex.quote(url)} --json state",
        "agents": mine[:limit],
        "agents_left_out": max(len(mine) - limit, 0),
        "artifacts": held[:limit],
        "artifacts_left_out": max(len(held) - limit, 0),
    }


def _instant(text: object) -> dt.datetime | None:
    """A time with a zone, or None when `text` is not one."""
    try:
        when = dt.datetime.fromisoformat(str(text))
    except ValueError:
        return None
    return when if when.tzinfo is not None else None


def _agents(board: Board, stopped: dt.datetime | None) -> tuple[list[Entry], int]:
    """Every agent record no run names, oldest first, each running or stalled, and how many could not be read.

    With `stopped`, an agent that started before it is stalled: its session
    stopped, so nothing will write its run.
    """
    ran = {
        str(row[0])
        for row in board.rows(
            f"SELECT json_extract(body, '$.agent') FROM records WHERE type = {literal(RUN_TYPE)}"  # noqa: S608 - values are quoted by literal()
            " AND json_extract(body, '$.agent') IS NOT NULL"
        )
    }
    rows = board.rows(
        f"SELECT logical, code, body FROM records WHERE type = {literal(AGENT_TYPE)} ORDER BY logical"  # noqa: S608 - values are quoted by literal()
    )
    found: list[Entry] = []
    unreadable = 0
    for path, code, body in rows:
        if path in ran:
            continue
        record = _object(body) if code is None or code in TREE_CODES else None
        started = _instant(record.get("started")) if record is not None else None
        if record is None or started is None:
            unreadable += 1
            continue
        found.append(
            {
                "path": path,
                "prompt": f"{path}.md",
                "session": record.get("session"),
                "cwd": record.get("cwd"),
                "started": record["started"],
                "works_on": record.get("works_on"),
                "state": "stalled" if stopped is not None and started < stopped else "running",
                "at": started,
            }
        )
    found.sort(key=lambda agent: (agent["at"], agent["path"]))
    return [{key: value for key, value in agent.items() if key != "at"} for agent in found], unreadable


def _pull_requests(board: Board) -> dict[str, str]:
    """Each change's pull request URL from its `opened` record, by the change's path."""
    rows = board.rows(
        f"SELECT logical, body FROM records WHERE type = {literal(OPENED_TYPE)} AND {READABLE}"  # noqa: S608 - values are quoted by literal()
    )
    found: dict[str, str] = {}
    for path, body in rows:
        record = _object(body)
        if record is not None and isinstance(record.get("pull_request"), str):
            found[str(path).rsplit("/", 1)[0]] = record["pull_request"]
    return found


def _artifacts(board: Board) -> dict[str, list[Entry]]:
    """Each artifact a version's `for` names, by each reference it names, newest version first.

    A version belongs to the artifact whose path is its own up to `/versions/`,
    and the artifact belongs to every `for` of its versions.
    """
    rows = board.rows(
        "SELECT logical, type, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type IN ({literal(ARTIFACT_TYPE)}, {literal(VERSION_TYPE)}) AND status = 'ok'"
    )
    urls: dict[str, str] = {}
    newest: dict[str, tuple[dt.datetime, str, Entry]] = {}
    belongs: dict[str, set[str]] = {}
    for path, kind, body in rows:
        record = _object(body)
        if record is None:
            continue
        if kind == ARTIFACT_TYPE:
            urls[str(path)] = str(record.get("url"))
            continue
        owner, at = str(path).rsplit(VERSIONS, 1)[0], _instant(record.get("at"))
        if at is None:
            continue
        for reference in cast("list[object]", record.get("for") or []):
            belongs.setdefault(str(reference), set()).add(owner)
        held = newest.get(owner)
        if held is None or (at, path) > (held[0], held[1]):
            newest[owner] = (at, str(path), record)
    found: dict[str, list[Entry]] = {}
    for reference, owners in belongs.items():
        held = sorted(((*newest[owner], owner) for owner in owners if owner in urls), reverse=True)
        found[reference] = [
            {"path": owner, "url": urls[owner], "title": record.get("title"), "at": record["at"]}
            for _, _, record, owner in held
        ]
    return found


def _waiting(board: Board, capability: str, key: str, limit: int) -> Entry:
    """The entries of a plugin's list of what waits, at most `limit`, or why it refused."""
    try:
        output = _call(board, capability, {"limit": limit})
    except PluginError as error:
        return {"entries": [], "total": 0, "left_out": 0, "unreadable": 0, "refused": _refusal(error)}
    entries = cast("list[Entry]", output[key])
    total = int(output["total"])
    unreadable = int(output.get("unreadable", 0))
    return {
        "entries": entries,
        "total": total,
        "left_out": total - len(entries),
        "unreadable": unreadable,
        "refused": None,
    }


def _object(body: object) -> Entry | None:
    """A record's body as a JSON object, or None."""
    try:
        parsed: object = json.loads(str(body))
    except ValueError:
        return None
    return cast("Entry", parsed) if isinstance(parsed, dict) else None
