"""`check`: every budget control, measured against the files it applies to."""

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


def check(request: CallRequest) -> CheckOutput:
    """Every budget control within its budget.

    A control Board marks degraded, such as one whose reference dangles, is
    `controls::control::degraded`; a control over its budget is
    `controls::budget::exceeded`. Each error names every control it applies to.
    A file reached by a reference and a pattern, or through a link, counts once.
    Each file is read once, however many controls reach it, with `board get`
    runs of up to 10,000 paths, and another run for paths left over a run's
    64 MiB output budget.
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
    files = board.referenced(reading, sorted(logical for logical, record in budgets.items() if record.applies_to))
    files.update({logical: set() for logical in budgets if logical not in files})
    for control, path, status, code in matched:
        if control in files:
            if status != "ok":
                raise _unreadable(control, path, code)
            files[control].add(path)
    distinct = sorted({path for paths in files.values() for path in paths})
    reads = dict(zip(distinct, reading.get(distinct), strict=True))
    checked = [_checked(logical, budgets[logical], sorted(files[logical]), reads) for logical in sorted(budgets)]
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


def _checked(logical: str, record: ControlRecord, files: list[str], reads: dict[str, bytes | PluginError]) -> Checked:
    rule = record.rule
    assert rule is not None  # noqa: S101 - check passes only budget controls
    used = 0
    for path in files:
        read = reads[path]
        if isinstance(read, PluginError):
            raise _unreadable(logical, path, read.fields.get("code"))
        used += measure(read)[rule.budget]
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
