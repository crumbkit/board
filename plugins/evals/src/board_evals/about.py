"""The `about` capability: the evals that name a subject or a repository's mount."""

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest

from board_evals.listing import listed
from board_evals.models import AboutInput, ListOutput


def about(request: CallRequest) -> ListOutput:
    """The evals whose `subjects` include the subject, or that name the mount in `mounts` or live under it.

    An eval lives under a mount when its path starts with the mount and a `/`. The prefix is compared with `substr`,
    so `_` and `%` in it match only themselves.
    """
    asked = AboutInput.model_validate(request.input)
    if asked.mount is not None:
        field, match = "$.mounts", f"s.value = {literal(asked.mount)}"
        prefix = literal(asked.mount + "/")
        inside = f" OR substr(logical, 1, length({prefix})) = {prefix}"
    else:
        field, match = "$.subjects", f"lower(s.value) = lower({literal(asked.subject or '')})"
        inside = ""
    condition = f"EXISTS (SELECT 1 FROM json_each(body, '{field}') AS s WHERE {match}){inside}"  # noqa: S608 - values are quoted by literal()
    return listed(Board(request.board.root, "evals"), condition, asked.limit)
