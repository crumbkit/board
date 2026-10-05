"""Board's plugin protocol: one request on stdin, one document on stdout.

Every document carries `$type`; every error a code `<plugin>::<area>::<reason>`.
The exit code is 0 with a result and 1 with an error. Logs go to stderr.
"""

import json
import sys
from collections.abc import Callable
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, JsonValue, TypeAdapter, ValidationError


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


class ExtractRecord(Strict):
    """One record to extract, by the blob that holds it."""

    blob: str
    path: str
    record_type: str = Field(alias="type")
    record: dict[str, JsonValue]


class ExtractRequest(Strict):
    """`board/extract/v1`."""

    tag: Literal["board/extract/v1"] = Field(alias="$type")
    records: list[ExtractRecord]
    board: BoardContext


REQUEST: TypeAdapter[CallRequest | ExtractRequest] = TypeAdapter(
    Annotated[CallRequest | ExtractRequest, Field(discriminator="tag")],
)


class PluginError(Exception):
    """A refusal Board reports with the plugin's code."""

    def __init__(self, code: str, message: str, fields: dict[str, JsonValue] | None = None) -> None:
        """Hold the code, the message and its typed fields."""
        super().__init__(message)
        self.code = code
        self.message = message
        self.fields: dict[str, JsonValue] = fields or {}


Call = Callable[[CallRequest], JsonValue]
Extract = Callable[[ExtractRequest], list[str]]


def serve(plugin: str, call: Call, stdin: bytes, extract: Extract | None = None) -> tuple[int, str]:
    """Answer one request for `plugin`: the exit code and the document to print.

    A plugin that passes no `extract` lists no types to extract, so Board never
    sends it `board/extract/v1`; such a request is refused like any malformed one.
    """
    try:
        request = REQUEST.validate_json(stdin)
        if isinstance(request, CallRequest):
            document: dict[str, JsonValue] = {"$type": "board/call-result/v1", "output": call(request)}
        elif extract is None:
            failure = PluginError(
                f"{plugin}::protocol::bad_request", f"{plugin} extracts nothing, so Board sends it no extraction"
            )
            return 1, _error(failure)
        else:
            document = {"$type": "board/extract-result/v1", "blobs": list(extract(request))}
    except ValidationError as error:
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
