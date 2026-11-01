"""Checking content at the boundary before it is sent, and recording what GitHub returned, in the personal board."""

import json
import subprocess
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import PluginError

# The fields Board adds to every plugin error, which it adds again to the error this plugin passes on.
ADDED = ("plugin", "stderr")
# How long the check may run, in seconds: the boundary's own limit, since judged rules wait on a model.
CHECK_TIMEOUT = 1800


def check(root: str, action: str, to: str, origin: str | None, parts: dict[str, str]) -> dict[str, Any]:
    """The output of `boundary.check` on the parts, or the boundary's refusal passed on.

    Board requires this plugin's codes to start with `github::`, so a refusal
    from the boundary is `github::boundary::refused`. Its message and fields
    are the boundary's own, `recovery` and its `Next:` step among them, with
    the boundary's code in `code`.
    """
    given: dict[str, object] = {"action": action, "to": to, "parts": parts}
    if origin is not None:
        given["from"] = origin
    command = ["board", "--personal", "--json", "call", "boundary.check"]
    try:
        ran = subprocess.run(  # noqa: S603 - the program is Board itself, found on PATH
            command,
            cwd=root,
            input=json.dumps(given).encode(),
            capture_output=True,
            timeout=CHECK_TIMEOUT + 60,
            check=False,
        )
        document = cast("dict[str, Any]", json.loads(ran.stdout))
    except (OSError, subprocess.TimeoutExpired, ValueError) as error:
        raise PluginError(
            "github::board::failed",
            f"`board --personal call boundary.check` gave no answer ({error}), so nothing was sent;"
            " check that the `board` first on PATH is this Board",
        ) from None
    if ran.returncode == 0:
        return cast("dict[str, Any]", document["data"]["output"])
    code, fields = str(document.get("code")), cast("dict[str, Any]", document.get("fields") or {})
    if code.startswith("boundary::"):
        kept: dict[str, object] = {key: value for key, value in fields.items() if key not in ADDED}
        raise PluginError("github::boundary::refused", str(document.get("message")), kept | {"code": code})
    raise PluginError(
        "github::board::refused",
        f"`board --personal call boundary.check` refused with {code}: {document.get('message')}; nothing was sent;"
        " when the code says a plugin is missing, enable boundary and controls in the personal board",
        {"code": code},
    )


def record(root: str, digest: str, result: str) -> None:
    """Record what GitHub returned on the crossing of `digest`."""
    Board(root, "github", flags=("--personal",)).document(
        "call", "boundary.record", "--digest", digest, "--result", result
    )
