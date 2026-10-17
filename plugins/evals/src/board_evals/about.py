"""The `about` capability: the evals that name a subject or a repository's mount."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_evals.listing import listed
from board_evals.models import AboutInput, ListOutput


def about(request: CallRequest) -> ListOutput:
    """The evals whose `subjects` include the subject, or that name the repo in `repos` or live under it.

    An eval lives under a repo when its path starts with the repo and a `/`. The prefix is compared with `substr`,
    so `_` and `%` in it match only themselves.
    """
    asked = AboutInput.model_validate(request.input)
    if asked.repo is not None:
        field, match = "$.repos", f"s.value = {literal(asked.repo)}"
        prefix = literal(asked.repo + "/")
        inside = f" OR substr(logical, 1, length({prefix})) = {prefix}"
    else:
        field, match = "$.subjects", f"lower(s.value) = lower({literal(asked.subject or '')})"
        inside = ""
    condition = f"EXISTS (SELECT 1 FROM json_each(body, '{field}') AS s WHERE {match}){inside}"  # noqa: S608 - values are quoted by literal()
    return listed(Board(request.board.root, "evals"), condition, asked.limit)
