"""`for-path`: the controls that apply to one path."""

from pathlib import Path

from board_controls import board
from board_controls.models import CONTROL_TYPE, Applying, ForPathInput, ForPathOutput
from board_controls.protocol import CallRequest


def for_path(request: CallRequest) -> ForPathOutput:
    """Each readable control whose `applies_to` reaches the path or whose `paths` match it, by the control's path.

    A pattern matches the path from the root of the file the path resolves
    to, through any link. A path with nothing there yet, such as a file about
    to be written, is matched as it is asked, without its leading `/`.
    """
    asked = ForPathInput.model_validate(request.input)
    root = request.board.root
    controls = board.controls(root)
    report = board.refs(root, asked.path)
    if report is None:
        reaching: set[str] = set()
        physical: str | None = asked.path.removeprefix("/")
    else:
        reaching = board.referencing(report)
        location = report["location"]
        # Patterns match only the board's own working tree, never a mount.
        own = location["source"] == "working-tree" and Path(location["repository"]).resolve() == Path(root).resolve()
        physical = location["path"] if own else None
    matched: set[str] = set()
    if physical is not None:
        rows = board.query(
            root,
            "SELECT DISTINCT r.logical FROM records r, json_each(r.body, '$.paths') p"  # noqa: S608 - values are quoted by literal()
            f" WHERE r.type = {board.literal(CONTROL_TYPE)} AND r.source = 'worktree'"
            f" AND {board.literal(physical)} GLOB p.value",
        )
        matched = {logical for (logical,) in rows}
    applying = (matched | reaching) & controls.keys()
    return ForPathOutput(
        controls=[
            Applying(
                control=logical,
                requirement=controls[logical].requirement,
                purpose=controls[logical].purpose,
                rule=controls[logical].rule,
            )
            for logical in sorted(applying)
        ],
    )
