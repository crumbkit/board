"""`check`: every budget control, measured against the files it applies to."""

from concurrent.futures import Future, ThreadPoolExecutor
from typing import TYPE_CHECKING, Literal

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_controls import board
from board_controls.models import CONTROL_TYPE, Checked, CheckInput, CheckOutput, ControlRecord

if TYPE_CHECKING:
    from pydantic import JsonValue


type Usage = dict[Literal["words", "bytes"], int]


def measure(data: bytes) -> Usage:
    """The words and bytes in `data`. Words are separated by any whitespace."""
    return {"words": len(data.decode(errors="replace").split()), "bytes": len(data)}


# Each read is a `board get` process. Past about eight at once, starting
# processes contends and reads get no faster.
READERS = 8


def _read(reading: Board, path: str) -> Usage | PluginError:
    """What the file at `path` uses, or why it could not be read."""
    try:
        return measure(reading.get(path))
    except PluginError as error:
        return error


def check(request: CallRequest) -> CheckOutput:
    """Every budget control within its budget.

    A control Board marks degraded, such as one whose reference dangles, is
    `controls::control::degraded`; a control over its budget is
    `controls::budget::exceeded`. Each error names every control it applies to.
    A file reached by a reference and a pattern, or through a link, counts once.
    Each file is read once, however many controls reach it, and several are
    read at a time. The time still grows with the number of files.
    """
    CheckInput.model_validate(request.input)
    reading = Board(request.board.root, "controls")
    broken = board.degraded(reading)
    if broken:
        names = ", ".join(f"{logical} ({code})" for logical, code in broken)
        raise PluginError(
            "controls::control::degraded",
            f"controls Board cannot use: {names}; run `board check` for why",
            {"controls": [{"control": logical, "code": code} for logical, code in broken]},
        )
    budgets = {logical: record for logical, record in board.controls(reading).items() if record.rule is not None}
    # A pointer's file holds where it leads, never what a budget counts.
    matched = reading.rows(
        "SELECT DISTINCT r.logical, e.logical, e.status, e.code"  # noqa: S608 - the one value is quoted by literal()
        " FROM records r, json_each(r.body, '$.paths') p, entries e"
        f" WHERE r.type = {literal(CONTROL_TYPE)} AND r.status = 'ok' AND r.source = 'worktree'"
        " AND e.source = 'worktree' AND e.kind != 'pointer' AND e.path GLOB p.value",
    )
    files: dict[str, set[str]] = {
        logical: board.referenced(reading, logical) if record.applies_to else set()
        for logical, record in budgets.items()
    }
    for control, path, status, code in matched:
        if control in files:
            if status != "ok":
                raise _unreadable(control, path, code)
            files[control].add(path)
    distinct = sorted({path for paths in files.values() for path in paths})
    pool = ThreadPoolExecutor(max_workers=READERS)
    try:
        reads = {path: pool.submit(_read, reading, path) for path in distinct}
        checked = [_checked(logical, budgets[logical], sorted(files[logical]), reads) for logical in sorted(budgets)]
    finally:
        # The first error, in control then file order, stops the reads still queued.
        pool.shutdown(cancel_futures=True)
    failing = [item for item in checked if item.used > item.max]
    if failing:
        names = ", ".join(f"{item.control} ({item.used} of {item.max} {item.budget})" for item in failing)
        over: list[JsonValue] = [item.model_dump(mode="json") for item in failing]
        raise PluginError(
            "controls::budget::exceeded",
            f"over budget: {names}; shorten the files, or raise the control's max in a change that says why",
            {"controls": over},
        )
    return CheckOutput(controls=checked)


def _checked(
    logical: str, record: ControlRecord, files: list[str], reads: dict[str, Future[Usage | PluginError]]
) -> Checked:
    rule = record.rule
    assert rule is not None  # noqa: S101 - check passes only budget controls
    used = 0
    for path in files:
        measured = reads[path].result()
        if isinstance(measured, PluginError):
            if measured.code != "controls::board::refused":
                raise measured
            raise _unreadable(logical, path, measured.fields.get("code"))
        used += measured[rule.budget]
    return Checked(
        control=logical,
        requirement=record.requirement,
        budget=rule.budget,
        max=rule.max,
        used=used,
        files=files,
    )


def _unreadable(control: str, path: str, code: object) -> PluginError:
    return PluginError(
        "controls::file::unreadable",
        f"{control} applies to {path}, which Board cannot read as a file ({code}); run `board check` for why,"
        " or change the control to name only files",
        {"control": control, "file": path, "code": code if isinstance(code, str) else None},
    )
