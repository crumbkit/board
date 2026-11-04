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
    """Why a git command gave no output.

    `started` is false when git could not be run at all, and `timed_out` is
    true when it ran past its timeout.
    """

    why: str
    stderr: str
    started: bool
    timed_out: bool = False


def git(repository: str, *arguments: str, stdin: bytes = b"", timeout: float = GIT_SECONDS) -> str | Failed:
    """What `git <arguments>` prints, run in `repository` with ISOLATED, or why it failed."""
    command = subcommand(arguments)
    try:
        ran = subprocess.run(  # noqa: S603 - each caller passes its own arguments
            ["git", *arguments],  # noqa: S607 - git is found on PATH
            cwd=repository,
            input=stdin,
            env=os.environ | ISOLATED,
            capture_output=True,
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        why = f"`git {command}` timed out after {timeout:g} s in {repository}"
        return Failed(why, "", started=True, timed_out=True)
    except FileNotFoundError as error:
        if not Path(repository).is_dir():
            return Failed(f"`git {command}` cannot run in {repository}: {error}", str(error), started=True)
        return Failed(f"git cannot be run: {error}; put git on PATH", str(error), started=False)
    except OSError as error:
        return Failed(f"git cannot be run: {error}; put git on PATH", str(error), started=False)
    if ran.returncode != 0:
        stderr = ran.stderr.decode(errors="replace").strip()[-4096:]
        return Failed(f"`git {command}` failed in {repository}: {stderr}", stderr, started=True)
    return ran.stdout.decode(errors="replace")


def subcommand(arguments: tuple[str, ...]) -> str:
    """The git command `arguments` run, after any `-c <name>=<value>` options before it."""
    at = 0
    while at < len(arguments) and arguments[at] == "-c":
        at += 2
    return arguments[at] if at < len(arguments) else "git"
