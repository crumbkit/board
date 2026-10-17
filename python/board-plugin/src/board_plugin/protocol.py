"""Board's plugin protocol: one request on stdin, one document on stdout.

Every document carries `$type`; every error a code `<plugin>::<area>::<reason>`.
The exit code is 0 with a result and 1 with an error. Logs go to stderr.
"""

import json
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass

type JsonValue = bool | int | float | str | list[JsonValue] | dict[str, JsonValue] | None


@dataclass(frozen=True)
class BoardContext:
    """Where the plugin is running: the board's root and its own database."""

    root: str
    database: str


@dataclass(frozen=True)
class CallRequest:
    """`board/call/v1`. Board has validated `input` against the capability's `inputSchema`."""

    capability: str
    input: dict[str, JsonValue]
    board: BoardContext


@dataclass(frozen=True)
class ExtractRecord:
    """One record to extract, by the blob that holds it. Board has validated it against its schema."""

    blob: str
    path: str
    record_type: str
    record: dict[str, JsonValue]


@dataclass(frozen=True)
class ExtractRequest:
    """`board/extract/v1`."""

    records: list[ExtractRecord]
    board: BoardContext


class BadRequestError(ValueError):
    """A request Board never sends."""


def parse(stdin: bytes) -> CallRequest | ExtractRequest:
    """The request Board wrote, or `BadRequestError`."""
    try:
        document = json.loads(stdin)
        board = BoardContext(**document["board"])
        if document["$type"] == "board/call/v1":
            return CallRequest(capability=document["capability"], input=document["input"], board=board)
        if document["$type"] == "board/extract/v1":
            records = [
                ExtractRecord(blob=item["blob"], path=item["path"], record_type=item["type"], record=item["record"])
                for item in document["records"]
            ]
            return ExtractRequest(records=records, board=board)
    except (ValueError, KeyError, TypeError) as error:
        raise BadRequestError(f"{type(error).__name__}: {error}") from None
    raise BadRequestError(f"$type is {document['$type']!r}, not board/call/v1 or board/extract/v1")


class PluginError(Exception):
    """A refusal Board reports with the plugin's code."""

    def __init__(self, code: str, message: str, fields: dict[str, object] | None = None) -> None:
        """Hold the code, the message and its typed fields."""
        super().__init__(message)
        self.code = code
        self.message = message
        self.fields: dict[str, object] = fields or {}


# Board validates the output against the capability's outputSchema, whose root is an object.
Call = Callable[[CallRequest], Mapping[str, object]]
Extract = Callable[[ExtractRequest], list[str]]


def serve(plugin: str, call: Call, stdin: bytes, extract: Extract | None = None) -> tuple[int, str]:
    """Answer one request for `plugin`: the exit code and the document to print.

    A plugin that passes no `extract` lists no types to extract, so Board never
    sends it `board/extract/v1`; such a request is refused like any malformed one.
    """
    try:
        request = parse(stdin)
        if isinstance(request, CallRequest):
            document: dict[str, object] = {"$type": "board/call-result/v1", "output": call(request)}
        elif extract is None:
            failure = PluginError(
                f"{plugin}::protocol::bad_request", f"{plugin} extracts nothing, so Board sends it no extraction"
            )
            return 1, _error(failure)
        else:
            document = {"$type": "board/extract-result/v1", "blobs": list(extract(request))}
    except BadRequestError as error:
        failure = PluginError(f"{plugin}::protocol::bad_request", f"the request is not one Board sends: {error}")
        return 1, _error(failure)
    except PluginError as failure:
        return 1, _error(failure)
    return 0, json.dumps(document)


def _error(failure: PluginError) -> str:
    return json.dumps(
        {"$type": "board/error/v1", "code": failure.code, "message": failure.message, "fields": failure.fields},
    )


def main(plugin: str, call: Call, extract: Extract | None = None) -> int:
    """Read the request from stdin, write the answer to stdout, return the exit code."""
    code, document = serve(plugin, call, sys.stdin.buffer.read(), extract)
    sys.stdout.write(document)
    return code


def dumped(document: dict[str, JsonValue]) -> str:
    """A document as a plugin's committed files hold it: sorted keys, two-space indents."""
    return json.dumps(document, indent=2, sort_keys=True) + "\n"
