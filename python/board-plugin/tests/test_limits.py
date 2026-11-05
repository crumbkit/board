"""Board's call limits against a real `uv run`."""

import json
import os
import shutil
import subprocess
import time
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[3]
FIXTURES = Path(__file__).resolve().parent / "fixtures"


def running(pid: str) -> bool:
    """Whether `pid` is running. A zombie is not: it holds nothing, and only waits for init to reap it."""
    state = subprocess.run(["ps", "-o", "stat=", "-p", pid], capture_output=True, text=True, check=False).stdout
    return not (state.strip() == "" or state.strip().startswith("Z"))


def test_a_call_past_its_stdout_limit_leaves_no_python_running_under_uv(tmp_path: Path) -> None:
    binary = Path(os.environ.get("BOARD_BIN", REPOSITORY / "target" / "debug" / "board")).resolve()
    uv = shutil.which("uv")
    assert uv is not None, "uv is on PATH"
    root, data = tmp_path / "board", tmp_path / "data"
    root.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    (root / ".board").mkdir()
    (root / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    plugins = data / "board" / "plugins"
    plugins.mkdir(parents=True)
    (plugins / "sleeper").symlink_to(FIXTURES / "sleeper")
    environment = {
        "PATH": os.pathsep.join([str(binary.parent), str(Path(uv).parent), "/usr/bin", "/bin"]),
        "HOME": os.environ.get("HOME", str(root)),
        "XDG_DATA_HOME": str(data),
        "XDG_CACHE_HOME": str(tmp_path / "cache"),
    }

    def board(*arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [str(binary), *arguments], cwd=root, env=environment, input="{}", capture_output=True, text=True, check=False
        )

    enabled = board("plugin", "enable", "sleeper")
    assert enabled.returncode == 0, enabled.stderr
    error = json.loads(board("--json", "call", "sleeper.sleep").stdout)
    assert error["code"] == "board::plugin::too_large", error
    pid = error["fields"]["stderr"].split()[-1]
    checks = 500
    while running(pid) and checks > 0:
        checks -= 1
        time.sleep(0.01)
    assert not running(pid), f"python {pid} outlived the call"
