"""The evals a query selects, listed within a limit. `about` and `open` differ only in their condition."""

from board_plugin.board import Board, literal
from pydantic import ValidationError

from board_evals.models import EVAL_TYPE, TREE_CODES, EvalRecord, Listed, ListOutput


def listed(board: Board, condition: str, limit: int) -> ListOutput:
    """The evals for which the SQL `condition` over `records.body` holds, by path, at most `limit`.

    One `board query` reads every match, in this board and in every mounted
    board, because a mounted record's path is its display path. A record
    Board marks degraded for anything but the tree is left out, as is one
    this plugin cannot read.
    """
    codes = ", ".join(literal(code) for code in TREE_CODES)
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - the condition is built from constants and literal()
        f" WHERE type = {literal(EVAL_TYPE)} AND (code IS NULL OR code IN ({codes}))"
        f" AND logical IS NOT NULL AND ({condition}) ORDER BY logical",
    )
    found: list[Listed] = []
    for path, body in rows:
        try:
            record = EvalRecord.model_validate_json(body)
        except ValidationError:
            # The board's own schema admits a record this plugin cannot read.
            continue
        found.append(
            Listed(
                path=path,
                question=record.question,
                subjects=record.subjects,
                status=record.status,
                verdict=record.verdict,
                decision=record.decision,
            )
        )
    return ListOutput(evals=found[:limit], total=len(found), truncated=len(found) > limit)
