"""Board's plugin protocol: one request on stdin, one document on stdout.

Every document carries `$type`; every error a code `controls::<area>::<reason>`.
The exit code is 0 with a result and 1 with an error. Logs go to stderr. The
plugin extracts nothing, so Board only ever sends it `board/call/v1`.
"""

import json
import sys
from collections.abc import Callable
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, JsonValue, ValidationError


class Strict(BaseModel):
    """A document that refuses fields it does not know."""

    model_config = ConfigDict(extra="forbid", frozen=True, populate_by_name=True)


class BoardContext(Strict):
    """Where the plugin is running: the board's root and its own database."""

    root: str
    database: str


class CallRequest(Strict):
    """`board/call/v1`."""

    tag: Literal["board/call/v1"] = Field(alias="$type")
    capability: str
    input: dict[str, JsonValue]
    board: BoardContext


class PluginError(Exception):
    """A refusal Board reports with this plugin's code."""

    def __init__(self, code: str, message: str, fields: dict[str, JsonValue] | None = None) -> None:
        """Hold the code, the message and its typed fields."""
        super().__init__(message)
        self.code = code
        self.message = message
        self.fields: dict[str, JsonValue] = fields or {}


Call = Callable[[CallRequest], JsonValue]


def serve(call: Call, stdin: bytes) -> tuple[int, str]:
    """Answer one request: the exit code and the document to print."""
    try:
        request = CallRequest.model_validate_json(stdin)
    except ValidationError as error:
        failure = PluginError("controls::protocol::bad_request", f"the request is not one Board sends: {error}")
        return 1, _error(failure)
    try:
        document: dict[str, JsonValue] = {"$type": "board/call-result/v1", "output": call(request)}
    except PluginError as failure:
        return 1, _error(failure)
    return 0, json.dumps(document)


def _error(failure: PluginError) -> str:
    return json.dumps(
        {"$type": "board/error/v1", "code": failure.code, "message": failure.message, "fields": failure.fields},
    )


def main(call: Call) -> int:
    """Read the request from stdin, write the answer to stdout, return the exit code."""
    code, document = serve(call, sys.stdin.buffer.read())
    sys.stdout.write(document)
    return code
