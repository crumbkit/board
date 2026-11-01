"""The `migrate` capability: rewrite each phase-based `changes/change/v1` as a `changes/change/v2` and its records.

A v1 change held `title`, `mount`, `phase`, `route`, and optionally `head`,
`plan` (a Markdown path), `pull_request`, `issue` and `records`. Its v2 form
keeps `title`, `route` and `issue`, takes `repo` from `mount`, the personal
workflow, and a branch and suite. The plan becomes a plan record whose
`text` references the old Markdown, which stays where it is, and the pull
request becomes an `opened` record. `phase`, `head` and `records` are
dropped: the steps are derived now, the head comes from checks, and the
files `records` listed stay in the change's directory.

The branch is the one branch whose tip, in the mounted clone, is the v1
`head`, and the suite is the first one the repository's contributing docs
name, as `start` finds it. `changes` in the input gives either for a change
whose record cannot supply it. A change at phase `dropped` gets a
`dropped` record, and one at phase `done` a `landed` record whose merge
commit `changes` must give. A change missing any of these is left as v1
and listed in `owed`.
"""

from typing import Any, cast

from board_plugin.board import Board
from board_plugin.migrate import Line, Old, olds, put
from board_plugin.migrate import migrate as written
from board_plugin.protocol import CallRequest
from board_plugin.repos import MOUNTS, git

from board_changes.change import (
    CHANGE_TYPE,
    DROPPED_TYPE,
    LANDED_TYPE,
    OPENED_TYPE,
    PLAN_TYPE,
    PLUGIN,
    WORKFLOW_TYPE,
    stamp,
)
from board_changes.repo import DOCS, files, suite_in
from board_changes.workflow import PERSONAL, SEEDS

OLD_TYPE = "changes/change/v1"
# Branches that never name a change.
DEFAULTS = frozenset({"HEAD", "main", "master"})
# Why a v1 change at phase `dropped` stopped, since v1 recorded no reason.
DROPPED_WHY = "Dropped before the change moved to work records; v1 recorded no reason."


def short(ref: str) -> str:
    """A branch's name from its full ref, such as `work` from `refs/heads/work` or `refs/remotes/origin/work`."""
    if ref.startswith("refs/heads/"):
        return ref.removeprefix("refs/heads/")
    return ref.removeprefix("refs/remotes/").split("/", 1)[-1]


def branch_at(directory: str, head: str) -> str | None:
    """The one branch, local or on origin, whose tip in `directory` is `head`; None when git fails or finds not one."""
    found = git(directory, "for-each-ref", "--points-at", head, "--format=%(refname)", "refs/heads", "refs/remotes")
    if found is None:
        return None
    names = {short(ref) for ref in found.split()} - DEFAULTS
    return next(iter(names)) if len(names) == 1 else None


def migrated(board: Board, old: Old, given: dict[str, str], clones: dict[str, str], when: str) -> list[Line] | str:
    """The lines that rewrite `old` as v2, or what the owner must give for it."""
    record = old.record
    repo = str(record["mount"])
    head = record.get("head")
    checkout = clones.get(repo)
    branch = given.get("branch") or (branch_at(checkout, head) if checkout and isinstance(head, str) else None)
    suite = given.get("suite") or (suite_in(board, repo, files(board, repo, DOCS)) if checkout else None)
    commit = given.get("commit")
    wanted = [("branch", branch), ("suite", suite), *([("commit", commit)] if record["phase"] == "done" else [])]
    missing = [name for name, value in wanted if value is None]
    if missing:
        fields = ", ".join(f'"{name}": "<{name}>"' for name in missing)
        return (
            f'{old.path} has no {" or ".join(missing)}; give `{{"changes": {{"{old.path}": {{{fields}}}}}}}` on stdin'
        )
    change: dict[str, Any] = {
        "$type": CHANGE_TYPE,
        "title": record["title"],
        "repo": repo,
        "branch": branch,
        "workflow": PERSONAL,
        "suite": suite,
        "route": record.get("route", "unread"),
    }
    if isinstance(record.get("issue"), str):
        change["issue"] = record["issue"]
    lines = [put(old, change)]
    if isinstance(record.get("plan"), str):
        plan: dict[str, object] = {"$type": PLAN_TYPE, "text": record["plan"], "uses": []}
        lines.append({"path": f"{old.path}/plans/{when}", "expect": "none", "record": plan})
    if isinstance(record.get("pull_request"), str):
        opened = {"$type": OPENED_TYPE, "pull_request": record["pull_request"]}
        lines.append({"path": f"{old.path}/opened", "expect": "none", "record": opened})
    if record["phase"] == "done":
        landed = {"$type": LANDED_TYPE, "commit": commit}
        lines.append({"path": f"{old.path}/landed", "expect": "none", "record": landed})
    if record["phase"] == "dropped":
        dropped = {"$type": DROPPED_TYPE, "why": DROPPED_WHY}
        lines.append({"path": f"{old.path}/dropped", "expect": "none", "record": dropped})
    return lines


def migrate(request: CallRequest) -> dict[str, object]:
    """Rewrite every v1 change that can be, with the personal workflow when it is missing, in one write."""
    board = Board(request.board.root, PLUGIN)
    given = cast("dict[str, dict[str, str]]", request.input.get("changes") or {})
    clones = {str(logical): str(clone) for logical, clone in board.rows(MOUNTS)}
    when = stamp()
    lines: list[Line] = []
    owed: list[str] = []
    for old in olds(board, [OLD_TYPE]):
        result = migrated(board, old, given.get(old.path, {}), clones, when)
        if isinstance(result, str):
            owed.append(result)
        else:
            lines.extend(result)
    if lines and board.refs([PERSONAL])[0] is None:
        seeded = {"$type": WORKFLOW_TYPE, "steps": SEEDS[PERSONAL]}
        lines.insert(0, {"path": PERSONAL, "expect": "none", "record": seeded})
    return written(board, lines, owed, apply=bool(request.input.get("apply")))
