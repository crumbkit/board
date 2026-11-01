"""The `open` capability: the changes that have not ended, with who holds each."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest
from board_plugin.repos import MOUNTS, REPO_TYPE, git, shown_remote

from board_changes.change import CHANGE_TYPE, CLAIM_TYPE, ENDS, PLUGIN, READABLE, TREE_CODES

# The default `limit` (STANDARDS 23).
OPEN_LIMIT = 100


def open_changes(request: CallRequest) -> dict[str, object]:
    """Every change with no landed or dropped record beneath it, by path, at most `limit`.

    `holder` keeps the changes whose newest claim names that session, `remote`
    those whose repository's remote has that stored form, and `branch` those
    on that branch.
    """
    asked = request.input
    limit = int(cast("int", asked.get("limit", OPEN_LIMIT)))
    board = Board(request.board.root, PLUGIN)
    rows = board.rows(
        f"SELECT logical, code, body FROM records WHERE type = {literal(CHANGE_TYPE)} ORDER BY logical"  # noqa: S608 - values are quoted by literal()
    )
    ends = ", ".join(literal(kind) for kind in ENDS.values())
    ended = {
        str(row[0]).rsplit("/", 1)[0]
        for row in board.rows(f"SELECT logical FROM records WHERE type IN ({ends})")  # noqa: S608 - values are quoted by literal()
    }
    claims = newest_claims(board)
    found: list[dict[str, object]] = []
    unreadable = 0
    for path, code, body in rows:
        if path in ended:
            continue
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        change = cast("dict[str, Any]", json.loads(body))
        claim = claims.get(path, {})
        entry: dict[str, object] = {key: change[key] for key in ("title", "repo", "branch", "route")}
        found.append(entry | {"path": path, "holder": claim.get("session"), "worktree": claim.get("worktree")})
    if "holder" in asked:
        found = [entry for entry in found if entry["holder"] == asked["holder"]]
    if "branch" in asked:
        found = [entry for entry in found if entry["branch"] == asked["branch"]]
    if "remote" in asked:
        wanted = shown_remote(str(asked["remote"]))
        known = remotes(board, {str(entry["repo"]) for entry in found})
        found = [entry for entry in found if known.get(str(entry["repo"])) == wanted]
    return {"changes": found[:limit], "total": len(found), "truncated": len(found) > limit, "unreadable": unreadable}


def newest_claims(board: Board) -> dict[str, dict[str, Any]]:
    """Each change's newest readable claim, by the change's path."""
    rows = board.rows(
        f"SELECT logical, body FROM records WHERE type = {literal(CLAIM_TYPE)} AND {READABLE} ORDER BY logical"  # noqa: S608 - values are quoted by literal()
    )
    # /changes/<name>/claims/<stamp> belongs to /changes/<name>; a later stamp replaces an earlier one.
    return {str(path).rsplit("/claims/", 1)[0]: cast("dict[str, Any]", json.loads(body)) for path, body in rows}


def remotes(board: Board, repos: set[str]) -> dict[str, str]:
    """The stored form of each repository's remote, by its path: its repo record's, else its mounted clone's origin."""
    if not repos:
        return {}
    listed = ", ".join(literal(repo) for repo in sorted(repos))
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(REPO_TYPE)} AND status = 'ok' AND logical IN ({listed})"
    )
    found = {str(path): str(json.loads(body)["remote"]) for path, body in rows}
    for path, repository in board.rows(MOUNTS):
        if path in repos and path not in found:
            origin = git(str(repository), "remote", "get-url", "origin")
            if origin:
                found[str(path)] = shown_remote(origin)
    return found
