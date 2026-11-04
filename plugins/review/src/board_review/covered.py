"""`covered`: whether reports cover every commit a pull request adds, by range or patch id.

The reports are those committed in the board's own tree, or those a change
attaches when the call names a change, whose commits are then its mount's.
"""

import json
from typing import Any, NotRequired, TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_review.git import Repository
from board_review.run import TREE_CODES
from board_review.shapes import REPORT_TYPE, CoveredOutput, Verdict

# How many uncovered commits a refusal names.
SHOWN = 20
# The record type of a change, which the changes plugin owns.
CHANGE_TYPE = "changes/change/v1"
# Findings at these severities shut the changes plugin's review gate, so an attached report with one never covers.
BLOCKING = ("critical", "high")


class _RangeReport(TypedDict):
    """What `covered` reads of a report of a range, which the schema gives patch ids."""

    verdict: Verdict
    range: str
    patch_ids: list[str]
    findings: list[dict[str, object]]


class _Change(TypedDict):
    """What `covered` reads of a `changes/change/v1`: the mount it changes, its head and what it attaches."""

    mount: str
    head: NotRequired[str]
    records: NotRequired[list[str]]


def covered(request: CallRequest) -> CoveredOutput:
    """The reports that cover every commit from the merge base of `base` and the head to the head.

    Commits that only add review records need no report, so committing a
    report does not leave a head that no report covers. A report with the
    verdict `changes` never covers. Another covers each commit its range
    contains. It covers by patch id, as after a rebase, only when every patch
    id it lists is a commit's in the range, so dropping a commit the review
    saw, such as one that undid part of another, needs a new review. A merge
    or an empty commit has no patch id, so only a range that contains it
    covers it. Only reports committed at HEAD in the board's own tree count,
    unless the call names a change: then only the reports the change
    attaches count, the commits are those of the clone its mount reads, and
    no commit is skipped, since the reports live in the board.
    """
    board = Board(request.board.root, "review")
    named = request.input.get("change")
    if named is None:
        repository = Repository(request.board.root)
        named_head = request.input.get("head", "HEAD")
        reports = _reports(board, repository)
    else:
        change = _change(board, str(named))
        repository = Repository(_clone(board, change["mount"]))
        named_head = request.input.get("head", change.get("head"))
        if named_head is None:
            raise PluginError(
                "review::change::no_head",
                f"the change at {named} records no head commit, so nothing says which commits to check; record"
                f" one with `board call changes.attach --path {named} --head <commit>`, or pass --head",
                {"change": named},
            )
        reports = _attached(board, change.get("records", []))
    head = repository.commit(str(named_head))
    base = repository.commit(str(request.input["base"]))
    merge_base = repository.merge_base(base, head)
    added = repository.commits(merge_base, head)
    commits = added if named is not None else [commit for commit in added if not repository.review_only(commit)]
    skipped = len(added) - len(commits)
    patch_ids = repository.patch_ids(commits)
    blocked = [path for path, report in reports if named is not None and _blocking(report)]
    counted = [(path, r) for path, r in reports if r["verdict"] != "changes" and path not in blocked]
    asking = sum(1 for _, report in reports if report["verdict"] == "changes")
    covering: list[str] = []
    uncovered = set(commits)
    present = set(patch_ids.values())
    for path, report in counted:
        listed = set(report["patch_ids"])
        by_id: set[str] = {c for c in uncovered if patch_ids.get(c) in listed} if listed <= present else set()
        reached = repository.within(report["range"]) | by_id
        if reached & set(commits):
            covering.append(path)
        uncovered -= reached
    if not uncovered:
        return {
            "head": head,
            "merge_base": merge_base,
            "skipped": skipped,
            "commits": len(commits),
            "reports": covering,
        }
    missing = [commit for commit in commits if commit in uncovered]
    reviewed = f"{merge_base}..{head}"
    if named is None:
        where = "in the board's own tree"
        fix = f"Run `board call review.run --brief <brief>` with a brief whose range is {reviewed}, and commit it"
    else:
        where = f"attached to {named}"
        fix = (
            f"Attach a report whose range is {reviewed} with `board call changes.attach --path {named} --ref <report>`"
        )
    raise PluginError(
        "review::head::uncovered",
        f"{len(missing)} of the {len(commits)} commits from {merge_base} to {head} are covered by no"
        f" review/report/v1 {where}"
        + (f", not counting {skipped} commits that only add review records" if skipped else "")
        + f"; of {len(reports)} reports, {asking} ask for changes"
        + (f" and {len(blocked)} have a critical or high finding" if named is not None else "")
        + f". The first is {missing[0]}. "
        + fix,
        {
            "head": head,
            "merge_base": merge_base,
            "skipped": skipped,
            "reports": len(reports),
            "uncovered": missing[:SHOWN],
        },
    )


def _reports(board: Board, repository: Repository) -> list[tuple[str, _RangeReport]]:
    """Every readable report of a range in the board's own tree that HEAD holds as it is on disk, by logical path.

    A report of a subject reviews a record, not commits, so it is left out.
    """
    codes = ", ".join(literal(code) for code in TREE_CODES)
    rows = board.rows(
        "SELECT logical, path, blob, body FROM records WHERE source = 'worktree'"  # noqa: S608 - literals quoted
        f" AND type = {literal(REPORT_TYPE)} AND (status = 'ok' OR code IN ({codes})) ORDER BY logical"
    )
    committed = repository.committed()
    reports = [
        (str(logical), json.loads(body)) for logical, path, blob, body in rows if committed.get(str(path)) == blob
    ]
    return [(logical, cast("_RangeReport", report)) for logical, report in reports if "range" in report]


def _change(board: Board, path: str) -> _Change:
    """The change at the logical `path`, or `review::change::not_found` when no readable change is there.

    A change Board marks degraded for anything but a broken reference counts as not there.
    """
    codes = ", ".join(literal(code) for code in TREE_CODES)
    rows = board.rows(
        f"SELECT body FROM records WHERE logical = {literal(path)} AND type = {literal(CHANGE_TYPE)}"  # noqa: S608 - literals quoted
        f" AND (status = 'ok' OR code IN ({codes}))"
    )
    if not rows:
        raise PluginError(
            "review::change::not_found",
            f"no readable {CHANGE_TYPE} is at {path}; name a change `board call changes.open` lists, or run"
            f" `board check {path}` to see why Board cannot read it",
            {"change": path},
        )
    return cast("_Change", json.loads(rows[0][0]))


def _clone(board: Board, mount: str) -> str:
    """The clone the mount at `mount` reads its pinned commit from, or `review::mount::not_found`."""
    [answer] = board.refs([mount])
    location: dict[str, Any] = answer["location"] if answer is not None else {}
    if location.get("source", "worktree") == "worktree":
        raise PluginError(
            "review::mount::not_found",
            f"no mount is at {mount}, so its commits cannot be read; name the repository by the logical path of"
            " its mount, such as /repos/<name>, and see what is mounted with `board ls /repos`",
            {"mount": mount},
        )
    return str(location["repository"])


def _attached(board: Board, refs: list[str]) -> list[tuple[str, _RangeReport]]:
    """Every readable report of a range among `refs`, in the order the change attaches them, as the gate reads them.

    The changes plugin's review gate counts the reports a change attaches as
    Board reads them, committed or not, so this does too.
    """
    if not refs:
        return []
    codes = ", ".join(literal(code) for code in TREE_CODES)
    listed = ", ".join(literal(ref) for ref in refs)
    rows = board.rows(
        f"SELECT logical, body FROM records WHERE logical IN ({listed}) AND type = {literal(REPORT_TYPE)}"  # noqa: S608 - literals quoted
        f" AND (status = 'ok' OR code IN ({codes}))"
    )
    found = {str(logical): cast("_RangeReport", json.loads(body)) for logical, body in rows}
    return [(ref, found[ref]) for ref in refs if ref in found and "range" in found[ref]]


def _blocking(report: _RangeReport) -> bool:
    """Whether `report` holds a finding that shuts the changes plugin's review gate, so it never covers a change."""
    return any(finding.get("severity") in BLOCKING for finding in report["findings"])
