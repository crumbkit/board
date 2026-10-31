"""`covered`: whether reports in the board's own tree cover every commit a pull request adds, by range or patch id."""

import json
from typing import TypedDict, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_review.git import Repository
from board_review.run import TREE_CODES
from board_review.shapes import REPORT_TYPE, CoveredOutput, Verdict

# How many uncovered commits a refusal names.
SHOWN = 20


class _RangeReport(TypedDict):
    """What `covered` reads of a report of a range, which the schema gives patch ids."""

    verdict: Verdict
    range: str
    patch_ids: list[str]


def covered(request: CallRequest) -> CoveredOutput:
    """The reports that cover every commit from the merge base of `base` and the head to the head.

    Commits that only add review records need no report, so committing a
    report does not leave a head that no report covers. A report with the
    verdict `changes` never covers. Another covers each commit its range
    contains. It covers by patch id, as after a rebase, only when every patch
    id it lists is a commit's in the range, so dropping a commit the review
    saw, such as one that undid part of another, needs a new review. A merge
    or an empty commit has no patch id, so only a range that contains it
    covers it. Only reports committed at HEAD in the board's own tree count.
    """
    repository = Repository(request.board.root)
    head = repository.commit(str(request.input.get("head", "HEAD")))
    base = repository.commit(str(request.input["base"]))
    merge_base = repository.merge_base(base, head)
    added = repository.commits(merge_base, head)
    commits = [commit for commit in added if not repository.review_only(commit)]
    skipped = len(added) - len(commits)
    patch_ids = repository.patch_ids(commits)
    reports = _reports(Board(request.board.root, "review"), repository)
    counted = [(path, report) for path, report in reports if report["verdict"] != "changes"]
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
    raise PluginError(
        "review::head::uncovered",
        f"{len(missing)} of the {len(commits)} commits from {merge_base} to {head} are covered by no"
        " review/report/v1 in the board's own tree"
        + (f", not counting {skipped} commits that only add review records" if skipped else "")
        + f"; of {len(reports)} reports, {len(reports) - len(counted)} ask for changes. The first is {missing[0]}."
        " Run `board call review.run --brief <brief>` with a brief whose range is"
        f" {merge_base}..{head}, and commit the report",
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
