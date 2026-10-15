"""The `about` capability: the evals that name a subject."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_evals.listing import listed
from board_evals.models import AboutInput, ListOutput


def about(request: CallRequest) -> ListOutput:
    """The evals whose `subjects` include the subject asked about, whatever their status."""
    asked = AboutInput.model_validate(request.input)
    condition = (
        "EXISTS (SELECT 1 FROM json_each(body, '$.subjects') AS s"  # noqa: S608 - the one value is quoted by literal()
        f" WHERE lower(s.value) = lower({literal(asked.subject)}))"
    )
    return listed(Board(request.board.root, "evals"), condition, asked.limit)
