"""The soft limit on work in progress: three active changes of a kind in build, per mount.

Build changes and prototypes are counted apart. A change counts while it is
active: within the last `BOARD_CHANGES_ACTIVE_DAYS` days, 3 by default, a
changes capability touched it, its head commit was committed, or a run it
references finished. An idle change and a paused change count toward
nothing. When the limit is reached, a move into build is refused unless the
caller gives a reason, which the change records and the output carries as a
warning.
"""

import json
import os
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from typing import TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.git import Failed, git
from board_plugin.protocol import PluginError

from board_changes.change import CHANGE_TYPE, OWN, TREE_CODES, Change, kind_of, referenced

# The most active changes of a kind in build per mount before a move into build needs a reason.
WIP_LIMIT = 3
# How many days a change stays active after its last touch, commit or run, unless the variable below says otherwise.
ACTIVE_DAYS = 3
ACTIVE_DAYS_VARIABLE = "BOARD_CHANGES_ACTIVE_DAYS"
RUN_TYPE = "runs/run/v1"
# The warning a move past the limit carries.
OVER_LIMIT = "changes::wip::over_limit"


class Idle(TypedDict):
    """An idle change in build, and what to do with it: sync, drop or resume."""

    path: str
    action: str
    command: str


class Limit(TypedDict):
    """The limit reached: the active changes holding places, the idle ones apart, and the override."""

    code: str
    mount: str
    kind: str
    limit: int
    active_days: int
    active: list[str]
    idle: list[Idle]
    override: str


def building(changes: list[Change], kind: str) -> list[Change]:
    """The changes of `kind` in build, which may hold a place; a prototype in build is always exploring."""
    return [change for change in changes if change["phase"] == "build" and kind_of(change) == kind]


def in_build(board: Board) -> dict[str, Change]:
    """Every change of the board's own in build that Board does not mark degraded, by path."""
    rows = board.rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CHANGE_TYPE)} AND json_extract(body, '$.phase') = 'build'"
        f" AND substr(logical, 1, {len(OWN)}) = {literal(OWN)} ORDER BY logical"
    )
    return {
        str(path): cast("Change", json.loads(body)) for path, code, body in rows if code is None or code in TREE_CODES
    }


def active_days() -> int:
    """The days a change stays active, from `BOARD_CHANGES_ACTIVE_DAYS`, or ACTIVE_DAYS when it is unset."""
    given = os.environ.get(ACTIVE_DAYS_VARIABLE, "")
    if not given:
        return ACTIVE_DAYS
    if not given.isdigit() or int(given) < 1:
        raise PluginError(
            "changes::wip::bad_active_days",
            f"{ACTIVE_DAYS_VARIABLE} is {given!r}, which is not a whole number of days of at least 1; "
            f"set it to one, such as 3, or unset it for {ACTIVE_DAYS}",
            {"variable": ACTIVE_DAYS_VARIABLE, "value": given},
        )
    return int(given)


def day_of(stamp: object) -> date | None:
    """The UTC day of an RFC 3339 time or a `YYYY-MM-DD` day, or `None` when it is neither."""
    if not isinstance(stamp, str):
        return None
    try:
        parsed = datetime.fromisoformat(stamp)
    except ValueError:
        return None
    return parsed.astimezone(UTC).date() if parsed.tzinfo else parsed.date()


def committed(board: Board, changes: dict[str, Change]) -> dict[str, date]:
    """The UTC day each change's head was committed, read with git from its mount's repository, when git knows it.

    git resolves each head on its own, so an abbreviated head names exactly
    one commit or none.
    """
    mounts = sorted({change["mount"] for change in changes.values() if "head" in change})
    repositories = {
        mount: (answer or {}).get("location", {}).get("repository")
        for mount, answer in zip(mounts, board.refs(mounts), strict=True)
    }
    found: dict[str, date] = {}
    for path, change in changes.items():
        repository = repositories.get(change["mount"])
        if "head" not in change or not isinstance(repository, str):
            continue
        out = git(repository, "log", "-1", "--format=%ct", "--end-of-options", change["head"])
        if not isinstance(out, Failed) and out.strip().isdigit():
            found[path] = datetime.fromtimestamp(int(out.strip()), UTC).date()
    return found


def finished(board: Board, changes: dict[str, Change]) -> dict[str, date]:
    """The UTC day the newest run each change references, through any link, finished."""
    found: dict[str, date] = {}
    for path, change in changes.items():
        records, _ = referenced(board, change.get("records", []))
        days = [
            day
            for type_name, body in records.values()
            if type_name == RUN_TYPE and (day := day_of(body.get("finished"))) is not None
        ]
        if days:
            found[path] = max(days)
    return found


def last_active(board: Board, changes: dict[str, Change]) -> dict[str, date | None]:
    """The newest day each change was touched, committed to or run, or `None` when nothing says."""
    commits, runs = committed(board, changes), finished(board, changes)
    newest: dict[str, date | None] = {}
    for path, change in changes.items():
        days = [day for day in (day_of(change.get("touched")), commits.get(path), runs.get(path)) if day is not None]
        newest[path] = max(days) if days else None
    return newest


def split(newest: dict[str, date | None], today: date, days: int) -> tuple[list[str], list[str]]:
    """The paths active on one of the `days` days up to `today`, today included, and the idle ones, in order."""
    since = today - timedelta(days=days - 1)
    active = [path for path, day in newest.items() if day is not None and day >= since]
    return active, [path for path in newest if path not in active]


def idle_action(path: str, change: Change) -> Idle:
    """What to do with an idle change: sync one with a pull request, resume one with commits, else drop it."""
    if "pull_request" in change:
        return Idle(path=path, action="sync", command="board call changes.sync")
    if "head" in change:
        return Idle(path=path, action="resume", command=f"board call changes.next --path {path}")
    return Idle(path=path, action="drop", command=f"board call changes.drop --path {path} --reason '<why>'")


@dataclass(frozen=True)
class Move:
    """A move into build as the limit reads it: the command that passes the limit, the reason given, and the window.

    `days` is how many days a change stays active, read from the
    environment by the capability.
    """

    override: str
    reason: object
    days: int


def admit(board: Board, path: str, change: Change, move: Move) -> Limit | None:
    """Whether `change` may move into build at `path`: `None` below the limit, else the limit it goes past.

    At the limit, without a reason the move is refused with
    `changes::wip::full`; with one, the change records it in `over_limit`
    and the returned limit is the warning to show.
    """
    kind, mount = kind_of(change), change["mount"]
    others = {
        other: held
        for other, held in in_build(board).items()
        if other != path and held["mount"] == mount and building([held], kind)
    }
    if len(others) < WIP_LIMIT:
        return None
    days = move.days
    active, idle = split(last_active(board, others), datetime.now(UTC).date(), days)
    if len(active) < WIP_LIMIT:
        return None
    limit = Limit(
        code=OVER_LIMIT,
        mount=mount,
        kind=kind,
        limit=WIP_LIMIT,
        active_days=days,
        active=active,
        idle=[idle_action(other, others[other]) for other in idle],
        override=move.override,
    )
    if isinstance(move.reason, str):
        change["over_limit"] = move.reason
        return limit
    what = "build changes" if kind == "build" else "prototypes"
    fields = {key: value for key, value in limit.items() if key != "code"}
    raise PluginError(
        "changes::wip::full",
        f"{len(active)} {what} active in the last {days} days are in build for {mount}, the limit of {WIP_LIMIT} "
        f"({', '.join(active)}), so {path} waits; finish, pause or drop one, deal with the idle ones in `idle`, "
        f"or go ahead and record why with `{move.override}`",
        {"path": path, **fields},
    )
