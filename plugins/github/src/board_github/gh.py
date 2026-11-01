"""Running gh, the GitHub CLI, which every capability that reaches GitHub goes through."""

import json
import subprocess
from typing import Any, cast

from board_plugin.protocol import PluginError

# How long gh may run, in seconds: less than Board's 30-second limit, so the plugin can report a stopped gh.
TIMEOUT = 20
# How much of gh's stderr a failure's message carries.
TAIL = 2000
# What gh prints on stderr when GitHub answers 404.
NOT_FOUND = "(HTTP 404)"


def api(endpoint: str) -> dict[str, Any] | None:
    """The JSON object `gh api <endpoint>` prints, or None when GitHub answers 404.

    Anything else that is not an object is `github::gh::failed`. Board sets an
    error's `stderr` field to the plugin's own, so gh's goes in the message.
    """
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["gh", "api", endpoint],  # noqa: S607 - gh is found on PATH, as the person runs it
            capture_output=True,
            timeout=TIMEOUT,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise failed(endpoint, f"did not finish: {error}") from None
    stderr = ran.stderr.decode(errors="replace").strip()
    if ran.returncode != 0:
        if NOT_FOUND in stderr:
            return None
        raise failed(endpoint, f"exited {ran.returncode}: {stderr[-TAIL:]}")
    try:
        answer: object = json.loads(ran.stdout)
    except ValueError:
        answer = None
    if not isinstance(answer, dict):
        raise failed(endpoint, "printed no JSON object")
    return cast("dict[str, Any]", answer)


def failed(endpoint: str, why: str, then: str = "check gh on PATH and `gh auth status`") -> PluginError:
    """`github::gh::failed` for `gh api <endpoint>`, saying `why` and what to do `then`."""
    return PluginError(
        "github::gh::failed",
        f"`gh api {endpoint}` {why}; {then}",
        {"endpoint": endpoint},
    )
