"""The soft limit on work in progress: active changes of a kind in build per repository, per owner or in the board.

One cap applies at a time: three per repository, unless the caps record
names another limit and scope. Build changes and prototypes are counted
apart, each against that cap. A change's repository is the clone Board says
its mount reads, keyed by that clone's origin in its stored form, so two
mounts of one repository count together. A clone whose origin git cannot
read is keyed by its path, and a mount Board cannot resolve by the mount
itself. A change counts while it is active: within the last `BOARD_CHANGES_ACTIVE_DAYS` days, 3 by default, a
changes capability touched it, its head commit was committed, or a run it
references finished. An idle change and a paused change count toward
nothing. When the limit is reached, a move into build is refused unless the
caller gives a reason, which the change records and the output carries as a
warning.
"""

import json
import os
import re
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from typing import NamedTuple, NotRequired, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.git import Failed, git
from board_plugin.protocol import PluginError
from board_plugin.repos import GITHUB, origin, shown_remote

from board_changes.change import CAPS_PATH, CAPS_TYPE, CHANGE_TYPE, OWN, TREE_CODES, Change, kind_of, referenced

# The most active changes of a kind in build in one repository, when there is no caps record, before a move into
# build needs a reason.
WIP_LIMIT = 3
# A GitHub URL through www.github.com or with a port, which `shown_remote` does not reduce: what precedes the host.
GITHUB_HOST = re.compile(r"^([a-z+]+://(?:[^@/]+@)?)(?:www\.)?github\.com(?::\d+)?/", re.IGNORECASE)
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
    """The limit reached: the cap, the active changes holding places, the idle ones apart, and the override.

    `repository` or `owner` names what the cap counted together when its
    scope is one of those; `unread` names each mount whose clone git read no
    origin for, and why.
    """

    code: str
    mount: str
    kind: str
    scope: str
    repository: NotRequired[str]
    owner: NotRequired[str]
    limit: int
    active_days: int
    active: list[str]
    idle: list[Idle]
    override: str
    unread: NotRequired[dict[str, str]]


class Repository(NamedTuple):
    """Where a change's mount reads: the repository's key, its owner, its clone, and why git gave no origin for it.

    The owner is the GitHub owner, or the key itself for a repository off GitHub.
    """

    key: str
    owner: str
    clone: str | None
    unread: str | None


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


def cap(board: Board) -> tuple[int, str]:
    """The cap's limit and scope: the caps record's, or WIP_LIMIT per repository without one.

    A caps record Board marks degraded is `changes::caps::degraded`, since its numbers cannot be trusted.
    """
    rows = board.rows(
        f"SELECT code, body FROM records WHERE logical = {literal(CAPS_PATH)} AND type = {literal(CAPS_TYPE)}"  # noqa: S608 - values are quoted by literal()
    )
    if not rows:
        return WIP_LIMIT, "repository"
    [[code, body]] = rows
    if code is not None:
        raise PluginError(
            "changes::caps::degraded",
            f"the caps record at {CAPS_PATH} is degraded with {code}; it needs both `limit` and `scope`; "
            f"run `board check {CAPS_PATH}` and fix it",
            {"path": CAPS_PATH, "code": code},
        )
    record = json.loads(body)
    return int(record["limit"]), str(record["scope"])


def repositories(board: Board, mounts: list[str]) -> dict[str, Repository]:
    """The repository each of `mounts` reads, by mount."""
    # Each mount's location is all that is read, so one row of what reaches it is enough.
    answers = board.refs(mounts, limit=1)
    found: dict[str, Repository] = {}
    for mount, answer in zip(mounts, answers, strict=True):
        located = (answer or {}).get("location", {}).get("repository")
        clone = located if isinstance(located, str) else None
        url, unread = origin(clone) if clone is not None else (None, None)
        key = shown_remote(GITHUB_HOST.sub(r"\1github.com/", url)).casefold() if url else clone or mount
        matched = GITHUB.fullmatch(key) if url else None
        found[mount] = Repository(key, matched[1] if matched else key, clone, unread)
    return found


def within(repository: Repository, scope: str) -> str | None:
    """What a change in `repository` shares with the others it counts with in `scope`; None for the board."""
    return {"repository": repository.key, "owner": repository.owner}.get(scope)


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


def committed(changes: dict[str, Change], read: dict[str, Repository]) -> dict[str, date]:
    """The UTC day each change's head was committed, read with git from its mount's clone in `read`, when git knows it.

    git resolves each head on its own, so an abbreviated head names exactly
    one commit or none.
    """
    found: dict[str, date] = {}
    for path, change in changes.items():
        clone = read[change["mount"]].clone
        if "head" not in change or clone is None:
            continue
        out = git(clone, "log", "-1", "--format=%ct", "--end-of-options", change["head"])
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


def last_active(board: Board, changes: dict[str, Change], read: dict[str, Repository]) -> dict[str, date | None]:
    """The newest day each change was touched, committed to or run, or `None` when nothing says."""
    commits, runs = committed(changes, read), finished(board, changes)
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

    The cap counts the active changes of the change's kind in build that
    share its repository, its owner, or the board, as the caps record's
    scope says. At the limit, without a reason the move is refused with
    `changes::wip::full`; with one, the change records it in `over_limit`
    and the returned limit is the warning to show.
    """
    kind, mount = kind_of(change), change["mount"]
    limit, scope = cap(board)
    others = {other: held for other, held in in_build(board).items() if other != path and building([held], kind)}
    if len(others) < limit:
        return None
    read = repositories(board, sorted({mount, *(held["mount"] for held in others.values())}))
    counted = within(read[mount], scope)
    others = {other: held for other, held in others.items() if within(read[held["mount"]], scope) == counted}
    if len(others) < limit:
        return None
    days = move.days
    active, idle = split(last_active(board, others, read), datetime.now(UTC).date(), days)
    if len(active) < limit:
        return None
    reached = Limit(
        code=OVER_LIMIT,
        mount=mount,
        kind=kind,
        scope=scope,
        limit=limit,
        active_days=days,
        active=active,
        idle=[idle_action(other, others[other]) for other in idle],
        override=move.override,
    )
    if counted is not None:
        reached["repository" if scope == "repository" else "owner"] = counted
    unread = {each: repo.unread for each, repo in read.items() if repo.unread is not None}
    if unread:
        reached["unread"] = unread
    if isinstance(move.reason, str):
        change["over_limit"] = move.reason
        return reached
    what = "build changes" if kind == "build" else "prototypes"
    where = "in the board" if counted is None else f"{'in' if scope == 'repository' else 'of'} {counted}"
    note = f"; git read no origin for {', '.join(unread)}, so each counts by its clone" if unread else ""
    fields = {key: value for key, value in reached.items() if key != "code"}
    raise PluginError(
        "changes::wip::full",
        f"{len(active)} {what} active in the last {days} days are in build {where}, the {scope} cap of {limit} "
        f"({', '.join(active)}), so {path} waits{note}; finish, pause or drop one, deal with the idle ones in "
        f"`idle`, or go ahead and record why with `{move.override}`",
        {"path": path, **fields},
    )
