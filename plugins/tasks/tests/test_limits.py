"""Board's call limits against a real `uv run`."""

import subprocess
import time

from conftest import FIXTURES, Board


def running(pid: str) -> bool:
    """Whether `pid` is running. A zombie is not: it holds nothing, and only waits for init to reap it."""
    state = subprocess.run(["ps", "-o", "stat=", "-p", pid], capture_output=True, text=True, check=False).stdout
    return not (state.strip() == "" or state.strip().startswith("Z"))


def test_a_call_past_its_stdout_limit_leaves_no_python_running_under_uv(board: Board) -> None:
    (board.data / "board" / "plugins" / "sleeper").symlink_to(FIXTURES / "sleeper")
    board.enable("tasks", "sleeper")
    error = board.json("call", "sleeper.sleep", stdin="{}")
    assert error["code"] == "board::plugin::too_large", error
    pid = error["fields"]["stderr"].split()[-1]
    checks = 500
    while running(pid) and checks > 0:
        checks -= 1
        time.sleep(0.01)
    assert not running(pid), f"python {pid} outlived the call"
