"""The `list` capability: the artifacts in the board with their newest version, newest first."""

import datetime

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest
from pydantic import ValidationError

from board_artifacts.models import (
    ARTIFACT_TYPE,
    VERSION_TYPE,
    ArtifactRecord,
    Current,
    Listed,
    ListInput,
    ListOutput,
    VersionRecord,
)

# Where a version record sits under its artifact's path.
VERSIONS = "/versions/"


def listed(request: CallRequest) -> ListOutput:
    """Every artifact record with its newest version record, newest first, at most `limit`.

    A version belongs to the artifact whose path is its own up to `/versions/`,
    so an artifact in a mounted board reads the same as one here. With `for`,
    only the artifacts with a version whose `for` names it are listed. Its newest
    version is the one recorded last, a tie going to the path that sorts last.
    """
    asked = ListInput.model_validate(request.input)
    rows = Board(request.board.root, "artifacts").rows(
        "SELECT logical, type, code, body FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type IN ({literal(ARTIFACT_TYPE)}, {literal(VERSION_TYPE)})"
    )
    artifacts: dict[str, ArtifactRecord] = {}
    newest: dict[str, tuple[VersionRecord, str]] = {}
    belongs: set[str] = set()
    unreadable = 0
    for path, kind, code, body in rows:
        if code is not None:
            unreadable += 1
            continue
        try:
            if kind == ARTIFACT_TYPE:
                artifacts[path] = ArtifactRecord.model_validate_json(body)
                continue
            version = VersionRecord.model_validate_json(body)
        except ValidationError:
            unreadable += 1
            continue
        owner = path.rsplit(VERSIONS, 1)[0]
        if asked.for_ is not None and asked.for_ in (version.for_ or []):
            belongs.add(owner)
        held = newest.get(owner)
        if held is None or (version.at, path) > (held[0].at, held[1]):
            newest[owner] = (version, path)
    found = [
        _entry(path, artifact, newest.get(path))
        for path, artifact in artifacts.items()
        if (asked.account is None or artifact.account == asked.account) and (asked.for_ is None or path in belongs)
    ]
    found.sort(key=_recency, reverse=True)
    return ListOutput(
        artifacts=found[: asked.limit],
        total=len(found),
        truncated=len(found) > asked.limit,
        unreadable=unreadable,
    )


def _entry(path: str, artifact: ArtifactRecord, current: tuple[VersionRecord, str] | None) -> Listed:
    return Listed(
        path=path,
        url=artifact.url,
        account=artifact.account,
        first_seen=artifact.first_seen,
        current=None
        if current is None
        else Current(path=current[1], version=current[0].version, title=current[0].title, at=current[0].at),
    )


def _recency(entry: Listed) -> tuple[datetime.datetime, str]:
    """When the artifact last changed in the board: its newest version, or when it was first seen."""
    return (entry.current.at if entry.current else entry.first_seen, entry.path)
