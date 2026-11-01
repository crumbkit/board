"""The protocol answers one request and refuses what Board would never send."""

import json

from board_plugin.protocol import CallRequest, JsonValue, PluginError, serve

BOARD = {"root": "/b"}


def echo(request: CallRequest) -> dict[str, JsonValue]:
    """A capability that returns its input, or refuses an input asking it to."""
    if request.input.get("refuse"):
        raise PluginError("demo::test::refused", "asked to refuse", {"why": "test"})
    if request.input.get("hint"):
        raise PluginError("demo::test::refused", "asked to refuse", hint="call again without hint")
    return request.input


def call(given: dict[str, JsonValue]) -> bytes:
    """A `board/call/v1` request holding `given`."""
    return json.dumps({"$type": "board/call/v1", "capability": "x", "input": given, "board": BOARD}).encode()


def test_a_call_is_answered_with_a_result_and_a_malformed_one_is_refused() -> None:
    code, document = serve("demo", echo, call({"a": 1}))
    assert (code, json.loads(document)) == (0, {"$type": "board/call-result/v1", "output": {"a": 1}})
    code, document = serve("demo", echo, call({"a": 1})[:-1])
    assert code == 1
    assert json.loads(document)["code"] == "demo::protocol::bad_request"


def test_a_request_of_another_type_is_refused_and_its_call_twin_is_answered() -> None:
    other = json.dumps({"$type": "board/extract/v1", "capability": "x", "input": {}, "board": BOARD}).encode()
    code, document = serve("demo", echo, other)
    assert (code, json.loads(document)["code"]) == (1, "demo::protocol::bad_request")
    code, document = serve("demo", echo, call({}))
    assert (code, json.loads(document)) == (0, {"$type": "board/call-result/v1", "output": {}})


def test_a_plugin_error_is_reported_with_its_code_and_fields() -> None:
    code, document = serve("demo", echo, call({"refuse": True}))
    assert code == 1
    assert json.loads(document) == {
        "$type": "board/error/v1",
        "code": "demo::test::refused",
        "message": "asked to refuse",
        "fields": {"why": "test"},
    }


def test_a_plugin_errors_hint_is_written_and_an_error_without_one_writes_none() -> None:
    code, document = serve("demo", echo, call({"hint": True}))
    assert (code, json.loads(document)) == (
        1,
        {
            "$type": "board/error/v1",
            "code": "demo::test::refused",
            "message": "asked to refuse",
            "fields": {},
            "hint": "call again without hint",
        },
    )
    code, document = serve("demo", echo, call({"refuse": True}))
    assert (code, "hint" in json.loads(document)) == (1, False)
