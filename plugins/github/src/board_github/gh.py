"""Running gh, the GitHub CLI, which every capability that reaches GitHub goes through."""

import json
import subprocess
from typing import Any, cast

from board_plugin.protocol import PluginError

from board_github.checkout import without_model_keys

# How long gh may run, in seconds: less than Board's 30-second limit, so the plugin can report a stopped gh.
TIMEOUT = 20
# How much of gh's stderr a failure's message carries.
TAIL = 2000
# What gh prints on stderr when GitHub answers 404.
NOT_FOUND = "(HTTP 404)"
# What a failure to send says, since gh may have sent the content before it failed.
UNSURE = "it may have been sent, so look on GitHub before you run it again"


def _run(arguments: list[str], stdin: bytes, cwd: str | None = None) -> subprocess.CompletedProcess[bytes]:
    """Run `gh <arguments>` in `cwd` with `stdin`, or raise OSError or TimeoutExpired."""
    return subprocess.run(  # noqa: S603 - the arguments are the plugin's own
        ["gh", *arguments],  # noqa: S607 - gh is found on PATH, as the person runs it
        cwd=cwd,
        env=without_model_keys(),
        input=stdin,
        capture_output=True,
        timeout=TIMEOUT,
        check=False,
    )


def api(endpoint: str) -> dict[str, Any] | None:
    """The JSON object `gh api <endpoint>` prints, or None when GitHub answers 404.

    Anything else that is not an object is `github::gh::failed`. Board sets an
    error's `stderr` field to the plugin's own, so gh's goes in the message.
    """
    try:
        ran = _run(["api", endpoint], b"")
    except (OSError, subprocess.TimeoutExpired) as error:
        raise failed(endpoint, f"did not finish: {error}") from None
    stderr = ran.stderr.decode(errors="replace").strip()
    if ran.returncode != 0:
        if NOT_FOUND in stderr:
            return None
        raise failed(endpoint, f"exited {ran.returncode}: {stderr[-TAIL:]}")
    answer = _object(ran.stdout)
    if answer is None:
        raise failed(endpoint, "printed no JSON object")
    return answer


def send(arguments: list[str], stdin: str, named: str, cwd: str | None = None) -> str:
    """What `gh <arguments>` prints in `cwd` after reading `stdin`, or `github::gh::failed` naming `named`."""
    try:
        ran = _run(arguments, stdin.encode(), cwd)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise unsent(named, f"did not finish: {error}") from None
    if ran.returncode != 0:
        stderr = ran.stderr.decode(errors="replace").strip()
        raise unsent(named, f"exited {ran.returncode}: {stderr[-TAIL:]}")
    return ran.stdout.decode(errors="replace")


def send_json(method: str, endpoint: str, body: dict[str, str]) -> dict[str, Any]:
    """The object GitHub answers `gh api --method <method> <endpoint>` with `body`; it holds an https html_url."""
    named = f"gh api --method {method} {endpoint}"
    printed = send(["api", "--method", method, endpoint, "--input", "-"], json.dumps(body), named)
    answer = _object(printed.encode())
    url = answer.get("html_url") if answer else None
    if answer is None or not isinstance(url, str) or not url.startswith("https://"):
        raise unsent(named, "answered without an https html_url")
    return answer


def _object(stdout: bytes) -> dict[str, Any] | None:
    """The JSON object gh printed, or None."""
    try:
        answer: object = json.loads(stdout)
    except ValueError:
        return None
    return cast("dict[str, Any]", answer) if isinstance(answer, dict) else None


def failed(endpoint: str, why: str, then: str = "check gh on PATH and `gh auth status`") -> PluginError:
    """`github::gh::failed` for `gh api <endpoint>`, saying `why` and what to do `then`."""
    return PluginError(
        "github::gh::failed",
        f"`gh api {endpoint}` {why}; {then}",
        {"endpoint": endpoint},
    )


def unsent(named: str, why: str) -> PluginError:
    """`github::gh::failed` for a command that sends content, which may have reached GitHub before it failed."""
    return PluginError(
        "github::gh::failed",
        f"`{named}` {why}; {UNSURE}, and check gh on PATH and `gh auth status`",
        {"command": named},
    )
