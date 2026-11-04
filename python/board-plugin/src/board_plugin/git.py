"""How every plugin runs git.

git runs without the user's or the system's config, never prompts, and gives a failure the plugin can report.
"""

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

# The variables that keep the user's and the system's git config out of a git command, and stop it prompting.
ISOLATED = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0"}
# How long a git command may take by default.
GIT_SECONDS = 60


@dataclass(frozen=True)
class Failed:
    """Why a git command gave no output: `started` is false when git could not be run at all."""

    why: str
    stderr: str
    started: bool


def git(
    repository: str, *arguments: str, stdin: bytes = b"", timeout: float = GIT_SECONDS, program: str = "git"
) -> str | Failed:
    """What `git <arguments>` prints, run in `repository` with ISOLATED, or why it failed."""
    try:
        ran = subprocess.run(  # noqa: S603 - each caller passes its own arguments
            [program, *arguments],
            cwd=repository,
            input=stdin,
            env=os.environ | ISOLATED,
            capture_output=True,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return Failed(f"`git {arguments[0]}` ran past {timeout:g} s in {repository}", "", started=True)
    except FileNotFoundError as error:
        if not Path(repository).is_dir():
            return Failed(f"`git {arguments[0]}` cannot run in {repository}: {error}", str(error), started=True)
        return Failed(f"git cannot be run: {error}; put git on PATH", str(error), started=False)
    except OSError as error:
        return Failed(f"git cannot be run: {error}; put git on PATH", str(error), started=False)
    if ran.returncode != 0:
        stderr = ran.stderr.decode(errors="replace").strip()[-4096:]
        return Failed(f"`git {arguments[0]}` failed in {repository}: {stderr}", stderr, started=True)
    return ran.stdout.decode(errors="replace")
