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
    """Where the plugin is running: the board's root."""

    root: str


@dataclass(frozen=True)
class CallRequest:
    """`board/call/v1`. Board has validated `input` against the capability's `inputSchema`."""

    capability: str
    input: dict[str, JsonValue]
    board: BoardContext


class BadRequestError(ValueError):
    """A request Board never sends."""


def parse(stdin: bytes) -> CallRequest:
    """The request Board wrote, or `BadRequestError`."""
    try:
        document = json.loads(stdin)
        board = BoardContext(root=document["board"]["root"])
        if document["$type"] == "board/call/v1":
            return CallRequest(capability=document["capability"], input=document["input"], board=board)
    except (ValueError, KeyError, TypeError) as error:
        raise BadRequestError(f"{type(error).__name__}: {error}") from None
    raise BadRequestError(f"$type is {document['$type']!r}, not board/call/v1")


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


def serve(plugin: str, call: Call, stdin: bytes) -> tuple[int, str]:
    """Answer one request for `plugin`: the exit code and the document to print."""
    try:
        document = {"$type": "board/call-result/v1", "output": call(parse(stdin))}
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


def main(plugin: str, call: Call) -> int:
    """Read the request from stdin, write the answer to stdout, return the exit code."""
    code, document = serve(plugin, call, sys.stdin.buffer.read())
    sys.stdout.write(document)
    return code


def dumped(document: dict[str, JsonValue]) -> str:
    """A document as a plugin's committed files hold it: sorted keys, two-space indents."""
    return json.dumps(document, indent=2, sort_keys=True) + "\n"
