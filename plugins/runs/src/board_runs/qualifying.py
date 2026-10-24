"""The runs of a suite that count: full runs of a clean revision, newest first. `latest` and `trend` read only these."""

from dataclasses import dataclass

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError
from pydantic import ValidationError

from board_runs.models import RUN_TYPE, SUITE_TYPE, TREE_CODES, RunRecord

# The command a refusal suggests for finding a suite's path.
LIST_SUITES = "board query \"SELECT logical FROM records WHERE type = 'runs/suite/v1'\""


@dataclass(frozen=True)
class Counted:
    """The qualifying runs by path, newest first, and how many runs were left out and why."""

    runs: list[tuple[str, RunRecord]]
    not_counted: int
    unreadable: int


def qualifying(board: Board, suite: str) -> Counted:
    """The runs in `<suite>/runs/` that are full and say they are clean, newest first, a tie going to the last path.

    A suite's runs are the records of type `runs/run/v1` under its `runs/`
    directory, read by display path, so a mounted board's suite works the
    same as one here. A run Board marks degraded for anything but a broken
    reference, or one this plugin cannot read, is counted as unreadable.
    """
    found = board.rows(
        f"SELECT logical FROM records WHERE type = {literal(SUITE_TYPE)} AND logical = {literal(suite)}"  # noqa: S608 - values are quoted by literal()
    )
    if not found:
        raise PluginError(
            "runs::suite::not_found",
            f"no runs/suite/v1 record is at {suite}; name a suite by its logical path, without .json, and list"
            f" the suites with `{LIST_SUITES}`",
            {"suite": suite},
        )
    prefix = literal(suite + "/runs/")
    rows = board.rows(
        "SELECT logical, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(RUN_TYPE)} AND substr(logical, 1, length({prefix})) = {prefix}",
    )
    runs: list[tuple[str, RunRecord]] = []
    not_counted = unreadable = 0
    for path, code, body in rows:
        if code is not None and code not in TREE_CODES:
            unreadable += 1
            continue
        try:
            run = RunRecord.model_validate_json(body)
        except ValidationError:
            # The board's own schema admits a record this plugin cannot read.
            unreadable += 1
            continue
        if run.scope == "full" and run.dirty is False:
            runs.append((path, run))
        else:
            not_counted += 1
    runs.sort(key=lambda found: (found[1].started, found[0]), reverse=True)
    return Counted(runs, not_counted, unreadable)
