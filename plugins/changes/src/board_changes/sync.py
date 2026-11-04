"""The `sync` and `drop` capabilities: a change ends when its pull request closes, or when it is dropped by hand.

`sync` reads each open change's pull request once with `gh api` and writes
every move in one commit, or, when gh fails, writes nothing. `open` and
`next` never run gh; they report the day of the last sync.
"""

import json
import re
import subprocess
from typing import Any, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import (
    CHANGE_TYPE,
    FINAL,
    OWN,
    PLUGIN,
    SYNC_PATH,
    SYNC_TYPE,
    TASK_TYPE,
    TREE_CODES,
    Change,
    read,
    today,
    write,
)
from board_changes.wip import idle_action

# A GitHub pull request's URL: its owner, repository and number.
PULL_URL = re.compile(r"^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/pull/([0-9]+)/?$")
# The phases a change can leave when its pull request closes.
SYNCED = ("build", "review", "land", "paused")
# How long one gh call may run.
GH_SECONDS = 20
# What gh prints on stderr when GitHub answers 404.
NOT_FOUND = "(HTTP 404)"
# How much of gh's stderr a failure's message carries.
STDERR_TAIL = 400
SYNC_COMMAND = "board call changes.sync"
# The default `limit` of `sync`'s entries (STANDARDS 23).
SYNC_LIMIT = 100


class Entry(TypedDict):
    """What sync found for one change and what it did: moved it to `to`, or left it and said why in `code`."""

    path: str
    phase: str
    to: str | None
    status: str
    code: str | None
    pull_request: str | None
    detail: str
    command: str | None


def last_sync(board: Board) -> dict[str, object]:
    """The day the last writing sync ran and whether that is before today, with the warning and command when it is."""
    rows = board.rows(
        f"SELECT json_extract(body, '$.synced') FROM records WHERE logical = {literal(SYNC_PATH)}"  # noqa: S608 - quoted by literal()
        f" AND type = {literal(SYNC_TYPE)}"
    )
    synced = str(rows[0][0]) if rows and rows[0][0] is not None else None
    stale = synced is None or synced < today()
    return {
        "synced": synced,
        "stale": stale,
        "code": "changes::sync::stale" if stale else None,
        "command": SYNC_COMMAND,
    }


def gh_failed(endpoint: str, why: str) -> PluginError:
    """`changes::gh::failed` for `gh api <endpoint>`, saying `why`; nothing was written."""
    return PluginError(
        "changes::gh::failed",
        f"`gh api {endpoint}` {why}, so sync wrote nothing; check gh on PATH and `gh auth status`, then run "
        f"`{SYNC_COMMAND}` again",
        {"endpoint": endpoint},
    )


def pull_state(owner: str, repo: str, number: str) -> dict[str, Any] | None:
    """What GitHub says of the pull request, or `None` when it answers 404."""
    endpoint = f"repos/{owner}/{repo}/pulls/{number}"
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["gh", "api", endpoint],  # noqa: S607 - gh is found on PATH
            capture_output=True,
            check=False,
            timeout=GH_SECONDS,
        )
    except subprocess.TimeoutExpired:
        raise gh_failed(endpoint, f"ran past {GH_SECONDS} s") from None
    except OSError as error:
        raise gh_failed(endpoint, f"could not run: {error}") from None
    stderr = ran.stderr.decode(errors="replace").strip()
    if ran.returncode != 0:
        if NOT_FOUND in stderr:
            return None
        raise gh_failed(endpoint, f"exited {ran.returncode}: {stderr[-STDERR_TAIL:]}")
    try:
        answer = json.loads(ran.stdout)
    except ValueError:
        answer = None
    if not isinstance(answer, dict):
        raise gh_failed(endpoint, "printed no JSON object")
    return cast("dict[str, Any]", answer)


def open_changes(board: Board) -> list[tuple[str, Change, str]]:
    """Each change of the board's own that a closing pull request can end, with its blob, by path."""
    phases = ", ".join(literal(phase) for phase in SYNCED)
    rows = board.rows(
        "SELECT logical, blob, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CHANGE_TYPE)} AND json_extract(body, '$.phase') IN ({phases})"
        f" AND substr(logical, 1, {len(OWN)}) = {literal(OWN)} ORDER BY logical"
    )
    return [
        (str(path), cast("Change", json.loads(body)), str(blob))
        for path, blob, code, body in rows
        if code is None or code in TREE_CODES
    ]


def open_tasks(board: Board, change: Change) -> list[str]:
    """The tasks the change references in `records` that are neither done nor dropped."""
    refs = change.get("records", [])
    if not refs:
        return []
    rows = board.rows(
        "SELECT logical, json_extract(body, '$.status') FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(TASK_TYPE)} AND logical IN ({', '.join(literal(ref) for ref in refs)})"
        " ORDER BY logical"
    )
    return [str(path) for path, status in rows if status not in ("done", "dropped")]


def entry(path: str, change: Change, status: str, detail: str, **given: str | None) -> Entry:
    """One entry of the sync's report."""
    return Entry(
        path=path,
        phase=change["phase"],
        to=given.get("to"),
        status=status,
        code=given.get("code"),
        pull_request=change.get("pull_request"),
        detail=detail,
        command=given.get("command"),
    )


def without_pull(path: str, change: Change) -> Entry:
    """The entry of a change that records no pull request: what to do with it."""
    action = idle_action(path, change)
    return entry(
        path,
        change,
        "no_pull_request",
        f"it records no pull request; {action['action']} it, or record its pull request with `board call "
        f"changes.attach --path {path} --pull_request <url>`",
        code="changes::sync::no_pull_request",
        command=action["command"],
    )


def synced(board: Board, path: str, change: Change) -> tuple[Entry, Change | None]:
    """What the change's pull request says, and the change to write when it closed, or `None` to leave it."""
    url = change.get("pull_request", "")
    matched = PULL_URL.match(url)
    if matched is None:
        detail = f"{url} is not a GitHub pull request URL, so its state cannot be read"
        return entry(path, change, "not_github", detail, code="changes::sync::not_github"), None
    owner, repo, number = matched.groups()
    answer = pull_state(owner, repo, number)
    drop = f"board call changes.drop --path {path} --reason '<why>'"
    if answer is None:
        detail = f"GitHub has no pull request at {url}; drop the change, or record the right URL"
        return entry(path, change, "not_found", detail, code="changes::sync::pull_not_found", command=drop), None
    if answer.get("merged") is True:
        owed = open_tasks(board, change)
        if owed:
            detail = (
                f"its pull request merged, but these tasks are not done or dropped: {', '.join(owed)}; close them "
                f"with `board call tasks.done`, then run `{SYNC_COMMAND}` again"
            )
            return entry(path, change, "owed", detail, code="changes::sync::owed", command=SYNC_COMMAND), None
        ended: Change = {**change, "phase": "done"}
        sha = answer.get("merge_commit_sha")
        if isinstance(sha, str) and re.fullmatch("[0-9a-f]{7,40}", sha):
            ended["merge_commit"] = f"https://github.com/{owner}/{repo}/commit/{sha}"
        return entry(path, change, "merged", f"its pull request {url} merged", to="done"), ended
    if answer.get("state") == "closed":
        closed = str(answer.get("closed_at") or "")[:10]
        on = f" on {closed}" if closed else ""
        reason = f"its pull request {url} was closed without merging{on}"
        ended = {**change, "phase": "dropped", "dropped_reason": reason}
        return entry(path, change, "closed", reason, to="dropped"), ended
    return entry(path, change, "open", f"its pull request {url} is open"), None


def sync(request: CallRequest) -> dict[str, object]:
    """Read each open change's pull request and end the change when it closed: done when merged, else dropped.

    With `dry_run`, report the same and write nothing. Otherwise every move
    and the day of the sync are written in one commit.
    """
    dry = request.input.get("dry_run") is True
    limit = int(cast("int", request.input.get("limit", SYNC_LIMIT)))
    board = Board(request.board.root, PLUGIN)
    entries: list[Entry] = []
    lines: list[dict[str, object]] = []
    for path, change, blob in open_changes(board):
        if "pull_request" not in change:
            entries.append(without_pull(path, change))
            continue
        found, ended = synced(board, path, change)
        entries.append(found)
        if ended is not None:
            lines.append({"path": path, "expect": blob, "record": ended})
    if not dry:
        rows = board.rows(f"SELECT blob FROM entries WHERE logical = {literal(SYNC_PATH)}")  # noqa: S608 - quoted by literal()
        expect = str(rows[0][0]) if rows else "none"
        lines.append({"path": SYNC_PATH, "expect": expect, "record": {"$type": SYNC_TYPE, "synced": today()}})
        write(board, lines)
    moved = sum(1 for found in entries if found["to"] is not None)
    return {
        "dry_run": dry,
        "changes": entries[:limit],
        "total": len(entries),
        "moved": moved,
        "truncated": len(entries) > limit,
    }


def drop(request: CallRequest) -> dict[str, object]:
    """End a change that is not done or dropped in `dropped`, recording why."""
    path, reason = str(request.input["path"]), str(request.input["reason"])
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    phase = change["phase"]
    if phase in FINAL:
        raise PluginError(
            "changes::phase::final",
            f"the change at {path} is already {phase}, so it cannot be dropped; list the open changes with "
            "`board call changes.open`",
            {"path": path, "phase": phase},
        )
    change |= {"phase": "dropped", "dropped_reason": reason}
    write(board, [{"path": path, "expect": blob, "record": change}])
    return {"path": path, "from": phase, "to": "dropped", "task": change.get("task")}
