"""The `latest` capability: the latest full run of a clean revision of a suite, and what it gave."""

import datetime

from board_plugin.board import Board
from board_plugin.protocol import CallRequest

from board_runs.models import FAILED, Latest, LatestInput, LatestOutput
from board_runs.qualifying import qualifying


def latest(request: CallRequest) -> LatestOutput:
    """The newest qualifying run, its outcome, measures, failed cases and age, or `null` when none qualifies."""
    asked = LatestInput.model_validate(request.input)
    counted = qualifying(Board(request.board.root, "runs"), asked.suite)
    if not counted.runs:
        return LatestOutput(run=None, not_counted=counted.not_counted, unreadable=counted.unreadable)
    path, run = counted.runs[0]
    # A case that failed in several trials is listed once.
    failed = list(dict.fromkeys(result.case for result in run.results if result.outcome in FAILED))
    ended = run.finished or run.started
    age = datetime.datetime.now(datetime.UTC) - ended
    return LatestOutput(
        run=Latest(
            path=path,
            started=run.started,
            finished=run.finished,
            outcome="fail" if failed else "pass",
            measures=run.measures or {},
            failed=failed,
            age_s=max(0, int(age.total_seconds())),
        ),
        not_counted=counted.not_counted,
        unreadable=counted.unreadable,
    )
