"""The `sync`, `close` and `drop` capabilities: a change ends when its pull request closes, or when dropped by hand.

`sync` reads each open change's pull request once with `gh api` and writes
every move in one commit, or, when gh fails, writes nothing. `close` does
the same for one named change and the pull request it is given. `open` and
`next` never run gh; they report the day of the last sync.
"""

import json
import re
import subprocess
import time
from typing import Any, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import (
    CHANGE_TYPE,
    FINAL,
    OWN,
    PLUGIN,
    SYNC_COMMAND,
    SYNC_PATH,
    SYNC_TYPE,
    TASK_TYPE,
    TREE_CODES,
    Change,
    read,
    referenced,
    today,
    write,
)
from board_changes.wip import idle_action

# A GitHub pull request's URL: its owner, repository and number.
PULL_URL = re.compile(r"^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/pull/([0-9]+)/?$")
# A full commit id, as GitHub gives a merge commit.
COMMIT = re.compile("^[0-9a-f]{40}$")
# The phases a change can leave when its pull request closes.
SYNCED = ("build", "review", "land", "paused")
# The phases whose gates a change that merged from them did not pass.
SKIPPED = {"build": "build and review", "review": "review"}
# How long one gh call may run.
GH_SECONDS = 20
# How long all of a sync's gh calls may run together: less than the capability's 600 seconds, so sync can say so.
SYNC_SECONDS = 540
# What gh prints on stderr when GitHub answers 404, which it also answers for a repository gh's account cannot see.
NOT_FOUND = "(HTTP 404)"
# The default `limit` of `sync`'s entries (STANDARDS 23).
SYNC_LIMIT = 100
GH_FAILED = "changes::sync::gh_failed"


class Entry(TypedDict):
    """What sync found for one change and what it did: moved it to `to`, or left it and said why in `code`."""

    path: str
    phase: str
    to: str | None
    status: str
    code: str | None
    pull_request: str | None
    task: str | None
    detail: str
    command: str | None


def gh_failed(endpoint: str, why: str) -> PluginError:
    """`changes::sync::gh_failed` for `gh api <endpoint>`, saying `why`; nothing was written."""
    return PluginError(
        GH_FAILED,
        f"`gh api {endpoint}` {why}, so nothing was written; check gh on PATH and `gh auth status`, then run "
        "the call again",
        {"endpoint": endpoint},
    )


def pull_state(owner: str, repo: str, number: str, deadline: float) -> dict[str, Any] | None:
    """What GitHub says of the pull request, or `None` when it answers 404; gh runs at most until `deadline`."""
    endpoint = f"repos/{owner}/{repo}/pulls/{number}"
    left = deadline - time.monotonic()
    if left <= 0:
        raise gh_failed(endpoint, f"was not run because sync's gh calls took {SYNC_SECONDS} s in all")
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["gh", "api", endpoint],  # noqa: S607 - gh is found on PATH
            capture_output=True,
            check=False,
            timeout=min(GH_SECONDS, left),
        )
    except subprocess.TimeoutExpired:
        raise gh_failed(endpoint, "ran past its time limit") from None
    except OSError as error:
        raise gh_failed(endpoint, f"could not run: {error}") from None
    stderr = ran.stderr.decode(errors="replace").strip()
    if ran.returncode != 0:
        if NOT_FOUND in stderr:
            return None
        raise gh_failed(endpoint, f"exited {ran.returncode}: {stderr}")
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
    """The tasks the change references in `records`, through any link, that are neither done nor dropped."""
    records, _ = referenced(board, change.get("records", []))
    return [
        path
        for path, (type_name, body) in records.items()
        if type_name == TASK_TYPE and body.get("status") not in ("done", "dropped")
    ]


def entry(path: str, change: Change, status: str, detail: str, **given: str | None) -> Entry:
    """One entry of the sync's report."""
    return Entry(
        path=path,
        phase=change["phase"],
        to=given.get("to"),
        status=status,
        code=given.get("code"),
        pull_request=change.get("pull_request"),
        task=change.get("task"),
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


def merged(board: Board, path: str, change: Change, url: str, answer: dict[str, Any]) -> tuple[Entry, Change | None]:
    """A merged pull request's entry, and the change ended in done, or `None` while a task is still open."""
    owed = open_tasks(board, change)
    if owed:
        detail = (
            f"its pull request merged, but these tasks are not done or dropped: {', '.join(owed)}; close them "
            f"with `board call tasks.done`, then run `{SYNC_COMMAND}` again"
        )
        return entry(path, change, "owed", detail, code="changes::sync::owed", command=SYNC_COMMAND), None
    ended: Change = {**change, "phase": "done"}
    detail = f"its pull request {url} merged"
    sha = answer.get("merge_commit_sha")
    if isinstance(sha, str) and COMMIT.match(sha):
        ended["merge_commit_url"] = f"{url.split('/pull/', maxsplit=1)[0]}/commit/{sha}"
    else:
        detail += ", and GitHub gave no merge commit, so none is recorded"
    if change["phase"] in SKIPPED:
        detail += f"; it left {change['phase']} without passing the {SKIPPED[change['phase']]} gates"
    return entry(path, change, "merged", detail, to="done"), ended


def synced(board: Board, path: str, change: Change, deadline: float) -> tuple[Entry, Change | None]:
    """What the change's pull request says, and the change to write when it closed, or `None` to leave it."""
    url = change.get("pull_request", "")
    attach = f"board call changes.attach --path {path} --pull_request <url>"
    matched = PULL_URL.match(url)
    if matched is None:
        detail = f"{url} is not a GitHub pull request URL, so its state cannot be read; record the right URL"
        return entry(path, change, "not_github", detail, code="changes::sync::not_github", command=attach), None
    owner, repo, number = matched.groups()
    answer = pull_state(owner, repo, number, deadline)
    if answer is None:
        detail = (
            f"GitHub answers 404 for {url}: no such pull request, or gh's account cannot see the repository; "
            "check `gh auth status`, then drop the change or record the right URL"
        )
        drop = f"board call changes.drop --path {path} --reason '<why>'"
        return entry(path, change, "not_found", detail, code="changes::sync::pull_not_found", command=drop), None
    if answer.get("merged") is True:
        return merged(board, path, change, url, answer)
    if answer.get("state") == "closed":
        closed = str(answer.get("closed_at") or "")[:10]
        on = f" on {closed}" if closed else ""
        reason = f"its pull request {url} was closed without merging{on}"
        ended: Change = {**change, "phase": "dropped", "dropped_reason": reason}
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
    deadline = time.monotonic() + SYNC_SECONDS
    entries: list[Entry] = []
    lines: list[dict[str, object]] = []
    for path, change, blob in open_changes(board):
        if "pull_request" not in change:
            entries.append(without_pull(path, change))
            continue
        found, ended = synced(board, path, change, deadline)
        entries.append(found)
        if ended is not None:
            lines.append({"path": path, "expect": blob, "record": ended})
    if not dry:
        rows = board.rows(f"SELECT blob FROM entries WHERE logical = {literal(SYNC_PATH)}")  # noqa: S608 - quoted by literal()
        expect = str(rows[0][0]) if rows else "none"
        lines.append({"path": SYNC_PATH, "expect": expect, "record": {"$type": SYNC_TYPE, "synced": today()}})
        write(board, lines)
    return {
        "dry_run": dry,
        "changes": entries[:limit],
        "total": len(entries),
        "moved": sum(1 for found in entries if found["to"] is not None),
        "truncated": len(entries) > limit,
    }


def close(request: CallRequest) -> dict[str, object]:
    """End one change by the pull request given, as `sync` would: done when it merged, else dropped.

    The change records the pull request in `pull_request`. Only a change in
    a phase `sync` reads may close; an open pull request is
    `changes::close::open`, and anything sync would leave alone is refused
    with the code sync gives it.
    """
    path, url = str(request.input["path"]), str(request.input["pr"])
    board = Board(request.board.root, PLUGIN)
    change, blob = read(board, path)
    phase = change["phase"]
    if phase not in SYNCED:
        raise PluginError(
            "changes::close::phase",
            f"the change at {path} is in {phase}, and only a change in {', '.join(SYNCED)} closes by its pull "
            "request; run `board call changes.next` for its next step, or `changes.drop` to end it",
            {"path": path, "phase": phase, "phases": list(SYNCED)},
        )
    found, ended = synced(board, path, {**change, "pull_request": url}, time.monotonic() + SYNC_SECONDS)
    if ended is None:
        code = found["code"] or "changes::close::open"
        detail = found["detail"] if found["code"] else f"its pull request {url} is still open"
        raise PluginError(
            code,
            f"{path} did not close: {detail}",
            {"path": path, "pull_request": url, "status": found["status"], "command": found["command"]},
        )
    write(board, [{"path": path, "expect": blob, "record": ended}])
    output: dict[str, object] = {"path": path, "from": phase, "to": ended["phase"], "pull_request": url}
    if "merge_commit_url" in ended:
        output["merge_commit_url"] = ended["merge_commit_url"]
    if "dropped_reason" in ended:
        output["dropped_reason"] = ended["dropped_reason"]
    return output


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
