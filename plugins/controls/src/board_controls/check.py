"""`check`: every budget control, measured against the files it applies to."""

from typing import TYPE_CHECKING

from board_controls import board
from board_controls.models import CONTROL_TYPE, Checked, CheckInput, CheckOutput, ControlRecord
from board_controls.protocol import CallRequest, PluginError

if TYPE_CHECKING:
    from pydantic import JsonValue


def measure(budget: str, data: bytes) -> int:
    """The words or bytes in `data`. Words are separated by any whitespace."""
    if budget == "bytes":
        return len(data)
    return len(data.decode(errors="replace").split())


def check(request: CallRequest) -> CheckOutput:
    """Every budget control within its budget.

    A control Board marks degraded, such as one whose reference dangles, is
    `controls::control::degraded`; a control over its budget is
    `controls::budget::exceeded`. Each error names every control it applies to.
    A file reached by a reference and a pattern, or through a link, counts once.
    """
    CheckInput.model_validate(request.input)
    root = request.board.root
    broken = board.degraded(root)
    if broken:
        names = ", ".join(f"{logical} ({code})" for logical, code in broken)
        raise PluginError(
            "controls::control::degraded",
            f"controls Board cannot use: {names}; run `board check` for why",
            {"controls": [{"control": logical, "code": code} for logical, code in broken]},
        )
    budgets = {logical: record for logical, record in board.controls(root).items() if record.rule is not None}
    # A pointer's file holds where it leads, never what a budget counts.
    matched = board.query(
        root,
        "SELECT DISTINCT r.logical, e.logical, e.status, e.code"  # noqa: S608 - the one value is quoted by literal()
        " FROM records r, json_each(r.body, '$.paths') p, entries e"
        f" WHERE r.type = {board.literal(CONTROL_TYPE)} AND r.status = 'ok' AND r.source = 'worktree'"
        " AND e.source = 'worktree' AND e.kind != 'pointer' AND e.path GLOB p.value",
    )
    files: dict[str, set[str]] = {
        logical: {board.display(root, path) for path in record.applies_to} for logical, record in budgets.items()
    }
    for control, path, status, code in matched:
        if control in files:
            if status != "ok":
                raise _unreadable(control, path, code)
            files[control].add(path)
    checked = [_checked(root, logical, budgets[logical], sorted(files[logical])) for logical in sorted(budgets)]
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


def _checked(root: str, logical: str, record: ControlRecord, files: list[str]) -> Checked:
    rule = record.rule
    assert rule is not None  # noqa: S101 - check passes only budget controls
    used = 0
    for path in files:
        try:
            used += measure(rule.budget, board.read(root, path))
        except PluginError as error:
            if error.code != "controls::board::refused":
                raise
            raise _unreadable(logical, path, error.fields.get("code")) from None
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
