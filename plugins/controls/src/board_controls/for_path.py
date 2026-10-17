"""`for-path`: the controls that apply to one path."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_controls import board
from board_controls.shapes import CONTROL_TYPE, Applying


def for_path(request: CallRequest) -> list[Applying]:
    """Each readable control whose `applies_to` reaches the path or whose `paths` match it, by the control's path.

    A pattern matches the file the path resolves to, through any link or
    mount, as `check` does: by the display path Board gives it, whichever
    path was asked. A path with nothing there yet, such as a file about to be
    written, is matched as asked, where Board would place a file there. A
    path beneath a file, where nothing can be, matches no pattern.
    """
    asked = str(request.input["path"])
    reading = Board(request.board.root, "controls")
    controls = board.controls(reading)
    [report] = reading.refs([asked], missing=True)
    matched: set[str] = set()
    if report is None:
        # Only a path beneath a file names nothing, and nothing can be written there.
        reaching: set[str] = set()
    else:
        reaching = board.referencing(report)
        location = report["location"]
        missing = report["missing"]
        # Only an existing file has a row in `files`; anything else is matched as asked.
        place = missing if missing is not None else location
        rows = reading.rows(
            f"WITH {board.FILES} SELECT DISTINCT r.logical FROM records r, json_each(r.body, '$.paths') p"  # noqa: S608 - values are quoted by literal()
            f" WHERE r.type = {literal(CONTROL_TYPE)} AND r.source = 'worktree' AND COALESCE("
            f"(SELECT full FROM files WHERE source = {literal(location['source'])}"
            f" AND path = {literal(place['path'])}),"
            f" {literal(place['display'][1:])}) GLOB p.value",
        )
        matched = {logical for (logical,) in rows}
    applying = (matched | reaching) & controls.keys()
    return [
        Applying(
            control=logical,
            requirement=controls[logical]["requirement"],
            purpose=controls[logical]["purpose"],
            rule=controls[logical].get("rule"),
        )
        for logical in sorted(applying)
    ]
