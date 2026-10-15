"""The `open` capability: the evals still planned or running."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_evals.listing import listed
from board_evals.models import OPEN, ListOutput, OpenInput


def open_evals(request: CallRequest) -> ListOutput:
    """The evals whose `status` is `planned` or `running`."""
    limit = OpenInput.model_validate(request.input).limit
    statuses = ", ".join(literal(status) for status in OPEN)
    return listed(Board(request.board.root, "evals"), f"json_extract(body, '$.status') IN ({statuses})", limit)
