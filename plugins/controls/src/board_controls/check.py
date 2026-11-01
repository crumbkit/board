"""`check`: the controls that apply under a path, each budget measured against the files it applies to."""

from typing import Any, Literal, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_controls import board, caller
from board_controls.shapes import LIMIT_DEFAULT, REPO_TYPE, Checked, CheckOutput, Counts, Cut, FileUsed, Stated

type Usage = dict[Literal["words", "bytes"], int]


def measure(data: bytes) -> Usage:
    """The words and bytes in `data`. Words are separated by any whitespace."""
    return {"words": len(data.decode(errors="replace").split()), "bytes": len(data)}


def check(request: CallRequest) -> CheckOutput:
    """The controls that apply to a file at or under the path asked, or to the files named beneath it.

    Without a path, the caller's repository is checked (caller.py); when it
    is none the board knows, nothing is. Under its board's root, every
    control of that board applies, even one that reaches no file. Under
    another path, only those reaching a file there do. With `files`, only
    those matching one of the named files do. A path in a mounted board also
    reads that board's own controls, whose patterns are anchored at the
    mount. A control with a rule is measured over all its files, wherever
    they are, and must be within its budget; one without a rule is stated,
    with its patterns and the number of its files under the path. A path that
    names a link or mount is checked where it leads. A control Board marks
    degraded, such as one whose reference dangles, is
    `controls::control::degraded`, under any path, since what it applies to
    is unknown; a measured control over its budget is
    `controls::budget::exceeded`. Each error names every control it applies
    to. A file reached by a reference and a pattern, or through a link,
    counts once. Each file is read once, however many controls reach it,
    with `board get` runs of up to 10,000 paths, and another run for paths
    left over a run's 64 MiB output budget. The files each control's
    patterns match come from `board query` pages of up to 10,000 rows. Each
    list is cut at `limit`, after every budget is checked.
    """
    reading = Board(request.board.root, "controls")
    given = request.input
    limit = int(cast("int", given.get("limit", LIMIT_DEFAULT)))
    path = given.get("path")
    if path is None:
        path, why = caller.repository(reading, request.board.root)
        if path is None:
            nothing = _listed(None, [], [], limit)
            nothing["next"] = why
            return nothing
    location = _location(reading, str(path))
    scope = str(location["display"])
    own = _own(location)
    _refuse_degraded(reading, own)
    controls = board.controls(reading, own)
    files, unreadable = _files(reading, controls, own)
    named = given.get("files")
    if named is None:
        reached = {logical: {file for file in files[logical] if _under(file, scope)} for logical in controls}
    else:
        reached = _named(reading, files, scope, own, cast("list[str]", named))
    # Under its board's root every control is listed, so one whose pattern reaches nothing shows with no files.
    everything = named is None
    applying = [
        logical for logical in sorted(controls) if reached[logical] or (everything and scope == controls[logical].board)
    ]
    budgets = [logical for logical in applying if controls[logical].record.get("rule") is not None]
    for logical in budgets:
        if logical in unreadable:
            raise _unreadable(logical, *unreadable[logical])
    distinct = sorted({file for logical in budgets for file in files[logical]})
    reads = dict(zip(distinct, reading.get(distinct), strict=True))
    measured = [_checked(logical, controls[logical], sorted(files[logical]), reads) for logical in budgets]
    failing = [item for item in measured if item["used"] > item["max"]]
    if failing:
        names = ", ".join(_over(item) for item in failing)
        raise PluginError(
            "controls::budget::exceeded",
            f"over budget: {names}; shorten the files, or raise the control's max in a change that says why",
            {"controls": failing},
        )
    stated = [
        _stated(logical, controls[logical], len(reached[logical])) for logical in applying if logical not in budgets
    ]
    _mark_superseded(reading, controls, [*measured, *stated])
    output = _listed(scope, measured, stated, limit)
    if own is not None:
        output["own_controls"] = "read"
    elif _unmounted(reading, scope):
        output["own_controls"] = "not_read"
        output["next"] = (
            f"{scope} is a repo record without a checkout, so the repository's own controls were not read;"
            " give the record a checkout to mount the repository (docs/architecture/paths/repos.md)"
        )
    return output


def _mark_superseded(reading: Board, controls: dict[str, board.Control], items: list[Checked | Stated]) -> None:
    """Add `basis_superseded` to each item whose control rests on a decision a decision in its board supersedes."""
    replaced = board.superseded(reading)
    for item in items:
        control = controls[item["control"]]
        basis = [ref for ref in item["basis"] if (control.source, ref) in replaced]
        if basis:
            item["basis_superseded"] = basis


def _refuse_degraded(reading: Board, own: board.Own | None) -> None:
    """Refuse the check when Board marks any control it reads degraded, naming each with its code."""
    broken = board.degraded(reading, own)
    if broken:
        names = ", ".join(f"{logical} ({code})" for logical, code in broken)
        raise PluginError(
            "controls::control::degraded",
            f"controls Board cannot use: {names}; run `board check` for why",
            {"controls": [{"control": logical, "code": code} for logical, code in broken]},
        )


def _files(
    reading: Board, controls: dict[str, board.Control], own: board.Own | None
) -> tuple[dict[str, set[str]], dict[str, tuple[str, object]]]:
    """Every file each control reaches, by reference or pattern, and for each the first one Board cannot read."""
    files = board.referenced(reading, controls)
    files.update({logical: set() for logical in controls if logical not in files})
    unreadable: dict[str, tuple[str, object]] = {}
    for control, found, status, code in board.matched(reading, own):
        if control in files:
            files[control].add(found)
            if status != "ok":
                unreadable.setdefault(control, (found, code))
    return files, unreadable


def _named(
    reading: Board, files: dict[str, set[str]], scope: str, own: board.Own | None, named: list[str]
) -> dict[str, set[str]]:
    """For each control, the named files it matches by pattern or reaches by reference.

    A named file is a path relative to `scope`, so it may be one the board
    does not hold, such as a file of a repository it does not mount.
    """
    paths = sorted({scope.rstrip("/") + "/" + file for file in named})
    hits = board.named(reading, paths, own)
    return {
        logical: {file for file in paths if (logical, file) in hits or file in reached}
        for logical, reached in files.items()
    }


def _stated(logical: str, control: board.Control, files: int) -> Stated:
    record = control.record
    return Stated(
        control=logical,
        board=control.board,
        requirement=record["requirement"],
        purpose=record["purpose"],
        paths=record.get("paths", []),
        files=files,
        adds_step=record.get("adds_step"),
        basis=record.get("basis", []),
    )


def _listed(scope: str | None, measured: list[Checked], stated: list[Stated], limit: int) -> CheckOutput:
    """The output listing at most `limit` of each of `measured` and `stated`, with their totals."""
    return CheckOutput(
        path=scope,
        measured=measured[:limit],
        stated=stated[:limit],
        total=Counts(measured=len(measured), stated=len(stated)),
        truncated=Cut(measured=len(measured) > limit, stated=len(stated) > limit),
    )


def _location(reading: Board, path: str) -> dict[str, Any]:
    """Where `path` resolves, through any link or mount."""
    if path == "/":
        return {"display": "/", "source": "worktree", "path": ""}
    [report] = reading.refs([path])
    if report is None:
        raise PluginError(
            "controls::board::refused",
            f"`board refs` refused with board::path::not_found: nothing is at {path};"
            " run `board ls /` to see what is there",
            {"code": "board::path::not_found"},
        )
    return cast("dict[str, Any]", report["location"])


def _own(location: dict[str, Any]) -> board.Own | None:
    """The mounted board a location is in, with the display path of its root; None in the board's own tree."""
    if location["source"] == "worktree":
        return None
    display, physical = str(location["display"]), str(location["path"])
    depth = len(physical.split("/")) if physical else 0
    root = "/".join(display.split("/")[: len(display.split("/")) - depth]) if depth else display
    return board.Own(source=str(location["source"]), root=root or "/")


def _unmounted(reading: Board, scope: str) -> bool:
    """Whether `scope` is a repo record in the board's own tree, which has no checkout since it is not a mount."""
    return bool(
        reading.rows(
            "SELECT 1 FROM entries"  # noqa: S608 - values are quoted by literal()
            f" WHERE source = 'worktree' AND kind = 'record' AND type = {literal(REPO_TYPE)}"
            f" AND logical = {literal(scope)}"
        )
    )


def _under(path: str, scope: str) -> bool:
    return scope in ("/", path) or path.startswith(scope + "/")


def _checked(logical: str, control: board.Control, files: list[str], reads: dict[str, bytes | PluginError]) -> Checked:
    record = control.record
    rule = record.get("rule")
    assert rule is not None  # noqa: S101 - check measures only budget controls
    usage: dict[str, int] = {}
    for path in files:
        read = reads[path]
        if isinstance(read, PluginError):
            raise _unreadable(logical, path, read.fields.get("code"))
        usage[path] = measure(read)[rule["budget"]]
    checked = Checked(
        control=logical,
        board=control.board,
        requirement=record["requirement"],
        budget=rule["budget"],
        max=rule["max"],
        used=sum(usage.values()),
        files=files,
        adds_step=record.get("adds_step"),
        basis=record.get("basis", []),
    )
    if rule.get("per", "total") == "total":
        return checked
    # The first of the sorted files with the most usage, so ties name the same file every run.
    largest = max(files, key=usage.__getitem__, default=None)
    over = [FileUsed(file=path, used=usage[path]) for path in files if usage[path] > rule["max"]]
    checked["used"] = max(usage.values(), default=0)
    checked["per"] = "file"
    checked["largest"] = largest
    checked["over"] = over
    return checked


def _over(item: Checked) -> str:
    """How a failing control is over its budget, naming each file when it limits files one by one."""
    if "over" not in item:
        return f"{item['control']} ({item['used']} of {item['max']} {item['budget']})"
    files = ", ".join(f"{over['file']} has {over['used']}" for over in item["over"])
    return f"{item['control']} (each file at most {item['max']} {item['budget']}: {files})"


def _unreadable(control: str, path: str, code: object) -> PluginError:
    return PluginError(
        "controls::file::unreadable",
        f"{control} applies to {path}, which Board cannot read as a file ({code}); run `board check` for why,"
        " or change the control to name only files",
        {"control": control, "file": path, "code": code if isinstance(code, str) else None},
    )
