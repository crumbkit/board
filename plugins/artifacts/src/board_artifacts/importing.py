"""The `import` capability: an artifact record for each listed artifact the board does not have."""

import datetime
import json

from board_plugin.board import Board
from board_plugin.protocol import CallRequest

from board_artifacts.models import (
    ARTIFACT_TYPE,
    ArtifactRecord,
    ImportInput,
    ImportOutput,
    Truncated,
    artifact_id,
)


def import_listing(request: CallRequest) -> ImportOutput:
    """Write, in one commit, an artifact record at `/artifacts/<id>` for each listed URL that has none.

    `first_seen` is the row's `updatedAt`, or the time of the import when the
    row has none. A path that holds anything already is left as it is. The
    records carry no version: reading each new artifact lets the mod record it.
    """
    asked = ImportInput.model_validate(request.input)
    board = Board(request.board.root, "artifacts")
    rows = {f"/artifacts/{artifact_id(row.url)}": row for row in asked.listing.artifacts}
    paths = list(rows)
    held = board.refs(paths)
    missing = [path for path, answer in zip(paths, held, strict=True) if answer is None]
    now = datetime.datetime.now(datetime.UTC).replace(microsecond=0)
    lines = [
        {
            "path": path,
            "expect": "none",
            "record": ArtifactRecord.model_validate(
                {
                    "$type": ARTIFACT_TYPE,
                    "url": rows[path].url,
                    "account": asked.account,
                    "first_seen": rows[path].updated_at or now,
                }
            ).model_dump(mode="json", by_alias=True),
        }
        for path in missing
    ]
    if lines:
        board.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in lines).encode())
    return ImportOutput(written=missing, known=len(paths) - len(missing), truncated=_truncated(asked))


def _truncated(asked: ImportInput) -> Truncated | None:
    """What a listing that says it is truncated left out, or None for a whole listing."""
    if asked.listing.truncated is not True:
        return None
    listed, total = len(asked.listing.artifacts), asked.listing.total
    of = f"of {total} " if total is not None else ""
    return Truncated(
        listed=listed,
        total=total,
        message=(
            f"the listing holds the {listed} most recently updated {of}artifacts of {asked.account}, and the Artifact"
            " tool lists no more, so the rest are not imported; a regular sync catches each one when it is updated"
        ),
    )
