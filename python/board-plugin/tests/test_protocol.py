"""The protocol answers one request and refuses what Board would never send."""

import json

from pydantic import JsonValue

from board_plugin.protocol import CallRequest, ExtractRequest, PluginError, serve

BOARD = {"root": "/b", "database": "/d.sqlite"}


def echo(request: CallRequest) -> JsonValue:
    """A capability that returns its input, or refuses an input asking it to."""
    if request.input.get("refuse"):
        raise PluginError("demo::test::refused", "asked to refuse", {"why": "test"})
    return request.input


def blobs(request: ExtractRequest) -> list[str]:
    """An extraction that handles every blob it is sent."""
    return [record.blob for record in request.records]


def call(given: dict[str, JsonValue]) -> bytes:
    """A `board/call/v1` request holding `given`."""
    return json.dumps({"$type": "board/call/v1", "capability": "x", "input": given, "board": BOARD}).encode()


EXTRACT = json.dumps(
    {
        "$type": "board/extract/v1",
        "records": [{"blob": "a1", "path": "/a", "type": "demo/a/v1", "record": {}}],
        "board": BOARD,
    }
).encode()


def test_a_call_is_answered_with_a_result_and_a_malformed_one_is_refused() -> None:
    code, document = serve("demo", echo, call({"a": 1}))
    assert (code, json.loads(document)) == (0, {"$type": "board/call-result/v1", "output": {"a": 1}})
    code, document = serve("demo", echo, call({"a": 1})[:-1])
    assert code == 1
    assert json.loads(document)["code"] == "demo::protocol::bad_request"


def test_an_extraction_is_refused_by_a_plugin_that_extracts_nothing_and_answered_by_one_that_does() -> None:
    code, document = serve("demo", echo, EXTRACT)
    assert (code, json.loads(document)["code"]) == (1, "demo::protocol::bad_request")
    code, document = serve("demo", echo, EXTRACT, blobs)
    assert (code, json.loads(document)) == (0, {"$type": "board/extract-result/v1", "blobs": ["a1"]})


def test_a_plugin_error_is_reported_with_its_code_and_fields() -> None:
    code, document = serve("demo", echo, call({"refuse": True}))
    assert code == 1
    assert json.loads(document) == {
        "$type": "board/error/v1",
        "code": "demo::test::refused",
        "message": "asked to refuse",
        "fields": {"why": "test"},
    }
