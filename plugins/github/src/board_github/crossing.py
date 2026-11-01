"""Checking content at the boundary before it is sent, and recording what GitHub returned, in the personal board."""

import json
from typing import Any

from board_plugin.board import Board
from board_plugin.protocol import PluginError

# The fields Board adds to every plugin error, which it adds again to the error this plugin passes on.
ADDED = ("plugin", "stderr")
# How long the check may run, in seconds: the boundary's own limit, since judged rules wait on a model, and a minute.
CHECK_TIMEOUT = 1860


def check(root: str, action: str, to: str, origin: str | None, parts: dict[str, str]) -> dict[str, Any]:
    """The output of `boundary.check` on the parts, or the boundary's refusal passed on.

    Board requires this plugin's codes to start with `github::`, so a refusal
    from the boundary is `github::boundary::refused`. Its message and fields
    are the boundary's own, `recovery` and its `Next:` step among them, with
    the boundary's code in `code` and any code the boundary carried in `cause`.
    """
    given: dict[str, object] = {"action": action, "to": to, "parts": parts}
    if origin is not None:
        given["from"] = origin
    personal = Board(root, "github", flags=("--personal",))
    try:
        report = personal.document("call", "boundary.check", stdin=json.dumps(given).encode(), timeout=CHECK_TIMEOUT)
    except PluginError as error:
        code = str(error.fields.get("code", ""))
        if error.code != "github::board::refused" or not code.startswith("boundary::"):
            raise
        message = error.message.removeprefix(f"`board --personal call` refused with {code}: ")
        kept = {key: value for key, value in error.fields.items() if key not in ADDED}
        raise PluginError("github::boundary::refused", message, kept) from None
    output: dict[str, Any] = report["data"]["output"]
    return output


def record(root: str, digest: str, result: str) -> None:
    """Record what GitHub returned on the crossing of `digest`."""
    Board(root, "github", flags=("--personal",)).document(
        "call", "boundary.record", "--digest", digest, "--result", result
    )


def covers(root: str, action: str, to: str, origin: str | None) -> list[str]:
    """The boundaries `boundary.covers` names for the action to the destination from the checkout."""
    flags = ["--action", action, "--to", to] + ([] if origin is None else ["--from", origin])
    report = Board(root, "github", flags=("--personal",)).document("call", "boundary.covers", *flags)
    boundaries: list[str] = report["data"]["output"]["boundaries"]
    return boundaries
