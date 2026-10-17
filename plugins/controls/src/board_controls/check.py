"""`check`: the controls that apply under a path, each budget measured against the files it applies to."""

from typing import Literal

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_controls import board
from board_controls.shapes import Checked, CheckOutput, ControlRecord, Stated

type Usage = dict[Literal["words", "bytes"], int]


def measure(data: bytes) -> Usage:
    """The words and bytes in `data`. Words are separated by any whitespace."""
    return {"words": len(data.decode(errors="replace").split()), "bytes": len(data)}


def check(request: CallRequest) -> CheckOutput:
    """The controls that apply to a file at or under the path asked, `/` when left out.

    Under `/`, every control applies, even one that reaches no file. Under
    another path, only those reaching a file there do.
    A control with a rule is measured over all its files, wherever they are,
    and must be within its budget; one without a rule is stated, with its files
    under the path. A path that names a link or mount is checked where it
    leads. A control Board marks degraded, such as one whose reference
    dangles, is `controls::control::degraded`, under any path, since what it
    applies to is unknown; a measured control over its budget is
    `controls::budget::exceeded`. Each error names every control it applies to.
    A file reached by a reference and a pattern, or through a link, counts once.
    Each file is read once, however many controls reach it, with `board get`
    runs of up to 10,000 paths, and another run for paths left over a run's
    64 MiB output budget. The files each control's patterns match come from
    `board query` pages of up to 10,000 rows.
    """
    reading = Board(request.board.root, "controls")
    broken = board.degraded(reading)
    if broken:
        names = ", ".join(f"{logical} ({code})" for logical, code in broken)
        raise PluginError(
            "controls::control::degraded",
            f"controls Board cannot use: {names}; run `board check` for why",
            {"controls": [{"control": logical, "code": code} for logical, code in broken]},
        )
    scope = _scope(reading, str(request.input.get("path", "/")))
    controls = board.controls(reading)
    files = board.referenced(
        reading, sorted(logical for logical, record in controls.items() if record.get("applies_to"))
    )
    files.update({logical: set() for logical in controls if logical not in files})
    unreadable: dict[str, tuple[str, object]] = {}
    for control, path, status, code in board.matched(reading):
        if control in files:
            files[control].add(path)
            if status != "ok":
                unreadable.setdefault(control, (path, code))
    # Under the root every control is listed, so one whose pattern reaches nothing shows with no files.
    applying = [
        logical for logical in sorted(controls) if scope == "/" or any(_under(path, scope) for path in files[logical])
    ]
    budgets = [logical for logical in applying if controls[logical].get("rule") is not None]
    for logical in budgets:
        if logical in unreadable:
            raise _unreadable(logical, *unreadable[logical])
    distinct = sorted({path for logical in budgets for path in files[logical]})
    reads = dict(zip(distinct, reading.get(distinct), strict=True))
    measured = [_checked(logical, controls[logical], sorted(files[logical]), reads) for logical in budgets]
    failing = [item for item in measured if item["used"] > item["max"]]
    if failing:
        names = ", ".join(f"{item['control']} ({item['used']} of {item['max']} {item['budget']})" for item in failing)
        raise PluginError(
            "controls::budget::exceeded",
            f"over budget: {names}; shorten the files, or raise the control's max in a change that says why",
            {"controls": failing},
        )
    stated = [
        Stated(
            control=logical,
            requirement=controls[logical]["requirement"],
            purpose=controls[logical]["purpose"],
            files=sorted(path for path in files[logical] if _under(path, scope)),
        )
        for logical in applying
        if logical not in budgets
    ]
    return CheckOutput(measured=measured, stated=stated)


def _scope(reading: Board, path: str) -> str:
    """The display path `path` resolves to, through any link or mount."""
    if path == "/":
        return path
    [report] = reading.refs([path])
    if report is None:
        raise PluginError(
            "controls::board::refused",
            f"`board refs` refused with board::path::not_found: nothing is at {path};"
            " run `board ls /` to see what is there",
            {"code": "board::path::not_found"},
        )
    return str(report["location"]["display"])


def _under(path: str, scope: str) -> bool:
    return scope in ("/", path) or path.startswith(scope + "/")


def _checked(logical: str, record: ControlRecord, files: list[str], reads: dict[str, bytes | PluginError]) -> Checked:
    rule = record.get("rule")
    assert rule is not None  # noqa: S101 - check measures only budget controls
    used = 0
    for path in files:
        read = reads[path]
        if isinstance(read, PluginError):
            raise _unreadable(logical, path, read.fields.get("code"))
        used += measure(read)[rule["budget"]]
    return Checked(
        control=logical,
        requirement=record["requirement"],
        budget=rule["budget"],
        max=rule["max"],
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
