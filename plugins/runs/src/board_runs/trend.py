"""The `trend` capability: one measure over the full runs of a clean revision of a suite, newest first."""

from board_plugin.board import Board
from board_plugin.protocol import CallRequest

from board_runs.models import MeasureValue, Point, RunRecord, TrendInput, TrendOutput
from board_runs.qualifying import qualifying


def trend(request: CallRequest) -> TrendOutput:
    """A point for each qualifying run that gives a number for the measure, at most `limit`, newest first."""
    asked = TrendInput.model_validate(request.input)
    counted = qualifying(Board(request.board.root, "runs"), asked.suite)
    points: list[Point] = []
    for path, run in counted.runs:
        value = measured(run, asked.measure, asked.case)
        # A flag is a bool, which Python also counts as an int; it is no point on a trend.
        if isinstance(value, int | float) and not isinstance(value, bool):
            points.append(Point(path=path, started=run.started, value=value))
    return TrendOutput(
        points=points[: asked.limit],
        total=len(points),
        truncated=len(points) > asked.limit,
        without_measure=len(counted.runs) - len(points),
        not_counted=counted.not_counted,
        unreadable=counted.unreadable,
    )


def measured(run: RunRecord, measure: str, case: str | None) -> MeasureValue | None:
    """The run's own value of `measure`, or the first result's for `case`; `None` when it gives none."""
    if case is None:
        return (run.measures or {}).get(measure)
    for result in run.results:
        if result.case == case:
            return (result.measures or {}).get(measure)
    return None
