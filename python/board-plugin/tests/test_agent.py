"""An agent started through `board_plugin.agent` is stopped with everything it started, and its answer is read."""

import os
import signal
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path

import pytest

from board_plugin import agent
from board_plugin.protocol import PluginError

SLEEP = 6


def example(argv: list[str]) -> agent.Agent:
    """An agent of the plugin `example` that runs `argv`."""
    return agent.Agent("example", "EXAMPLE_AGENT", argv)


def test_an_agent_past_its_time_is_stopped_and_one_within_it_answers() -> None:
    sleeper = ["sh", "-c", f"sleep {SLEEP}; echo late"]
    started = time.monotonic()
    with pytest.raises(PluginError) as refused:
        agent.ask(example(sleeper), "", 0.2, "shorten it")
    assert refused.value.code == "example::agent::timeout"
    assert "shorten it, or make the agent EXAMPLE_AGENT names answer sooner" in refused.value.message
    assert time.monotonic() - started < SLEEP / 2, "the sleep the agent started was not stopped with it"
    assert agent.ask(example(["sh", "-c", "echo soon"]), "", 5, "shorten it") == "soon\n"


def soon(condition: Callable[[], bool]) -> bool:
    """Whether `condition` holds within five seconds."""
    deadline = time.monotonic() + 5
    while not condition():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)
    return True


def gone(pid: int) -> bool:
    """Whether the process `pid` stops within five seconds."""

    def stopped() -> bool:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        return False

    return soon(stopped)


@pytest.mark.parametrize("signum", [signal.SIGTERM, signal.SIGINT, signal.SIGHUP])
def test_a_signal_that_stops_the_plugin_stops_the_agent_and_what_it_started(tmp_path: Path, signum: int) -> None:
    pidfile = tmp_path / "pid"
    command = ["sh", "-c", f"sleep {SLEEP} & echo $! > {pidfile}; wait"]
    script = f"from board_plugin.agent import Agent, ask; ask(Agent('example', 'X', {command!r}), '', {SLEEP}, 'x')"
    plugin = subprocess.Popen([sys.executable, "-c", script])
    assert soon(lambda: pidfile.exists() and pidfile.read_text().strip() != "")
    plugin.send_signal(signum)
    assert plugin.wait(timeout=10) == 128 + signum
    assert gone(int(pidfile.read_text()))


def test_a_process_the_agent_leaves_behind_is_stopped_when_the_agent_exits() -> None:
    out = agent.ask(example(["sh", "-c", f"sleep {SLEEP} >/dev/null 2>&1 & echo $!"]), "", 5, "shorten it")
    assert gone(int(out))


def test_an_agent_whose_escaped_child_holds_its_output_is_stopped_after_the_drain(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(agent, "DRAIN", 0.5)
    pidfile = tmp_path / "pid"
    escape = (
        "import subprocess, sys, time\n"
        f"sleeper = [sys.executable, '-c', 'import time; time.sleep({SLEEP})']\n"
        "child = subprocess.Popen(sleeper, start_new_session=True)\n"
        f"open({str(pidfile)!r}, 'w').write(str(child.pid))\n"
        f"time.sleep({SLEEP})\n"
    )
    started = time.monotonic()
    with pytest.raises(PluginError) as refused:
        agent.ask(example([sys.executable, "-c", escape]), "", 1, "shorten it")
    assert refused.value.code == "example::agent::timeout"
    assert time.monotonic() - started < SLEEP / 2, "the plugin waited for the escaped child's output"
    os.kill(int(pidfile.read_text()), signal.SIGKILL)


def test_an_agent_that_cannot_start_is_refused_naming_its_variable_and_one_that_can_runs() -> None:
    with pytest.raises(PluginError) as refused:
        agent.ask(example(["/nonexistent/agent"]), "", 5, "shorten it")
    assert refused.value.code == "example::agent::not_started"
    assert "change EXAMPLE_AGENT" in refused.value.message
    assert agent.ask(example(["sh", "-c", "echo ran"]), "", 5, "shorten it") == "ran\n"
