"""The protocol answers one request and refuses what Board would never send."""

import json

from pydantic import JsonValue

from board_controls.protocol import CallRequest, PluginError, serve


def echo(request: CallRequest) -> JsonValue:
    """A capability that returns its input, or refuses an input asking it to."""
    if request.input.get("refuse"):
        raise PluginError("controls::test::refused", "asked to refuse", {"why": "test"})
    return request.input


def request(given: dict[str, JsonValue]) -> bytes:
    """A `board/call/v1` request holding `given`."""
    board = {"root": "/b", "database": "/d.sqlite"}
    return json.dumps({"$type": "board/call/v1", "capability": "x", "input": given, "board": board}).encode()


def test_a_request_is_answered_with_a_result_and_a_malformed_one_is_refused() -> None:
    code, document = serve(echo, request({"a": 1}))
    assert (code, json.loads(document)) == (0, {"$type": "board/call-result/v1", "output": {"a": 1}})
    code, document = serve(echo, request({"a": 1})[:-1])
    assert code == 1
    assert json.loads(document)["code"] == "controls::protocol::bad_request"


def test_a_plugin_error_is_reported_with_its_code_and_fields() -> None:
    code, document = serve(echo, request({"refuse": True}))
    assert code == 1
    assert json.loads(document) == {
        "$type": "board/error/v1",
        "code": "controls::test::refused",
        "message": "asked to refuse",
        "fields": {"why": "test"},
    }
