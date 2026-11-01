"""A clean-context model agent: the argv a user names in an environment variable, given a prompt on stdin.

The review and controls plugins start their agents through this module, so
it alone sets the signal handlers that stop an agent with the plugin.
"""

import contextlib
import json
import os
import signal
import subprocess
import tempfile
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.protocol import PluginError

# How much of the agent's stdout or stderr an error carries.
TAIL = 4096
# How long to read the agent's output after its group is killed. A process that left the group may hold it open.
DRAIN = 5
# The signals that stop the plugin, and with it the agent: Board's SIGTERM, Ctrl-C and a closed terminal.
STOPPING = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)


@dataclass(frozen=True)
class Agent:
    """The agent `variable` names for `plugin`, as its argv."""

    plugin: str
    variable: str
    argv: list[str]


def command(plugin: str, variable: str) -> Agent:
    """The agent whose argv `variable` holds as a JSON array of strings, or `<plugin>::agent::not_set` or `invalid`."""
    text = os.environ.get(variable)
    if text is None:
        raise PluginError(
            f"{plugin}::agent::not_set",
            f"{variable} is not set; set it to the agent's argv as a JSON array of strings, as README.md shows",
            {"variable": variable},
        )
    try:
        parsed: object = json.loads(text)
    except ValueError:
        parsed = None
    parts = cast("list[object]", parsed) if isinstance(parsed, list) else []
    # An argument may be empty, as in `--tools ""`; the program may not.
    if not parts or not parts[0] or not all(isinstance(part, str) for part in parts):
        raise PluginError(
            f"{plugin}::agent::invalid",
            f"{variable} is {text[:200]!r}, which is not a JSON array of strings whose first names a program;"
            ' set it to one, such as ["claude", "-p"]',
            {"variable": variable},
        )
    return Agent(plugin, variable, [str(part) for part in parts])


def ask(agent: Agent, prompt: str, timeout: float, hint: str) -> str:
    """What the agent printed on stdout, given `prompt` on stdin in a new empty directory.

    `hint` is what to try first when the agent runs past `timeout`, such as
    "narrow the brief's range".

    The empty directory holds no project settings or MCP servers. The agent
    leads its own process group. The group is killed when the agent exits,
    after `timeout` seconds, or when the plugin gets one of `STOPPING`, so
    neither the agent nor a process it started outlives the call.
    """
    with tempfile.TemporaryDirectory(prefix=f"board-{agent.plugin}-") as empty:
        try:
            child = subprocess.Popen(  # noqa: S603 - the user's own environment names the agent
                agent.argv,
                cwd=empty,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
            )
        except OSError as error:
            raise PluginError(
                f"{agent.plugin}::agent::not_started",
                f"the agent {agent.argv[0]!r} cannot be started: {error}; change {agent.variable}",
                {"command": agent.argv},
            ) from error
        return _wait(agent, child, prompt, timeout, hint)


def _wait(agent: Agent, child: "subprocess.Popen[bytes]", prompt: str, timeout: float, hint: str) -> str:
    plugin, program = agent.plugin, agent.argv[0]

    def stop(signum: int, _frame: object) -> None:
        _kill(child.pid)
        raise SystemExit(128 + signum)

    previous = {signum: signal.signal(signum, stop) for signum in STOPPING}
    try:
        out, err = child.communicate(prompt.encode(), timeout=timeout)
    except subprocess.TimeoutExpired:
        _kill(child.pid)
        out = _drain(child)
        raise PluginError(
            f"{plugin}::agent::timeout",
            f"the agent {program!r} ran past {timeout:g} seconds and was stopped; {hint},"
            f" or make the agent {agent.variable} names answer sooner",
            {"seconds": timeout, "stdout": out.decode(errors="replace")[-TAIL:]},
        ) from None
    finally:
        _kill(child.pid)
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    stdout = out.decode(errors="replace")
    if child.returncode != 0:
        stderr = err.decode(errors="replace")[-TAIL:]
        raise PluginError(
            f"{plugin}::agent::failed",
            f"the agent {program!r} exited {child.returncode}; its stdout ends: {stdout.strip()[-200:]}; its"
            f" stderr ends: {stderr.strip()[-200:]}",
            {"exit": child.returncode, "stderr": stderr, "stdout": stdout[-TAIL:]},
        )
    return stdout


def _drain(agent: "subprocess.Popen[bytes]") -> bytes:
    """What the killed agent printed on stdout, read for at most `DRAIN` seconds."""
    try:
        out, _ = agent.communicate(timeout=DRAIN)
    except subprocess.TimeoutExpired:
        for stream in (agent.stdout, agent.stderr):
            if stream is not None:
                stream.close()
        agent.wait()
        return b""
    return out


def _kill(group: int) -> None:
    """Kill every process in the agent's group, which may already have exited.

    macOS refuses with EPERM a group whose processes have all exited but are not yet reaped.
    """
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(group, signal.SIGKILL)


def envelope(stdout: str) -> tuple[str, dict[str, int], list[str]]:
    """The agent's answer, its tokens and models: from `claude -p --output-format json`'s envelope, else none."""
    try:
        parsed: object = json.loads(stdout)
    except ValueError:
        return stdout, {}, []
    if not isinstance(parsed, dict) or not isinstance(cast("dict[str, Any]", parsed).get("result"), str):
        return stdout, {}, []
    found = cast("dict[str, Any]", parsed)
    usage = found.get("usage")
    tokens: dict[str, int] = {}
    if isinstance(usage, dict):
        counts = {key: value for key, value in cast("dict[str, Any]", usage).items() if isinstance(value, int)}
        inputs = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens")
        tokens = {
            "input_tokens": sum(counts.get(key, 0) for key in inputs),
            "output_tokens": counts.get("output_tokens", 0),
        }
    models = found.get("modelUsage")
    names = sorted(cast("dict[str, Any]", models)) if isinstance(models, dict) else []
    return str(found["result"]), tokens, names


def last_object(answer: str, key: str) -> dict[str, Any] | None:
    """The last JSON object in `answer` that holds `key`, or None."""
    decoder = json.JSONDecoder()
    start = answer.rfind("{")
    while start != -1:
        try:
            found, _ = decoder.raw_decode(answer, start)
        except ValueError:
            found = None
        if isinstance(found, dict) and key in found:
            return cast("dict[str, Any]", found)
        start = answer.rfind("{", 0, start)
    return None
