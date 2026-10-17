"""The `about` capability: the evals that name a subject or a repository's mount."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_evals.listing import listed
from board_evals.models import AboutInput, ListOutput


def about(request: CallRequest) -> ListOutput:
    """The evals whose `subjects` include the subject, or whose `repos` include the repo, whatever their status."""
    asked = AboutInput.model_validate(request.input)
    if asked.repo is not None:
        field, match = "$.repos", f"s.value = {literal(asked.repo)}"
    else:
        field, match = "$.subjects", f"lower(s.value) = lower({literal(asked.subject or '')})"
    condition = f"EXISTS (SELECT 1 FROM json_each(body, '{field}') AS s WHERE {match})"  # noqa: S608 - values are quoted by literal()
    return listed(Board(request.board.root, "evals"), condition, asked.limit)
