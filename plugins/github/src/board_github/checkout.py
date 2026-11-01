"""The checkout Board was started in: its GitHub repository, its mount, its branch, and what a push would send."""

import os
import re
import subprocess

from board_plugin.board import Board
from board_plugin.protocol import PluginError

# How long one git command may run, in seconds. A push or ls-remote reaches the network.
TIMEOUT = 300
# How much of git's stderr a failure's message carries.
TAIL = 2000
# A GitHub remote in the https or ssh forms, with `github.com-<alias>` ssh hosts, as owner and repository.
GITHUB = re.compile(
    r"(?:https://(?:[^@/]+@)?github\.com/|ssh://(?:[^@/]+@)?github\.com(?:-[\w.-]+)?/|[^@/:]+@github\.com(?:-[\w.-]+)?:)"
    r"([A-Za-z0-9-]+)/([A-Za-z0-9._-]+?)(?:\.git)?/?"
)
# The short SHA that names a commit's parts, at a fixed length so a commit's part names never change.
SHORT = 12


def git(top: str, *arguments: str, stdin: str = "") -> str:
    """What `git <arguments>` prints in `top`, or `github::git::failed` naming the command and git's error."""
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["git", "-C", top, *arguments],  # noqa: S607 - git is found on PATH
            input=stdin,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=TIMEOUT,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise _git_failed(arguments, f"did not finish: {error}") from None
    if ran.returncode != 0:
        raise _git_failed(arguments, f"exited {ran.returncode}: {ran.stderr.strip()[-TAIL:]}")
    return ran.stdout


def _git_failed(arguments: tuple[str, ...], why: str) -> PluginError:
    command = " ".join(["git", *arguments[:2]])
    return PluginError("github::git::failed", f"`{command}` {why}; check git and the checkout", {"command": command})


def top() -> str:
    """The root of the git checkout Board was started in, which Board names in BOARD_CALLER_DIR."""
    directory = os.environ.get("BOARD_CALLER_DIR", "")
    ran = subprocess.run(  # noqa: S603 - this reads the checkout only
        ["git", "-C", directory or ".", "rev-parse", "--show-toplevel"],  # noqa: S607 - git is found on PATH
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
        check=False,
    )
    if not directory or ran.returncode != 0:
        raise PluginError(
            "github::checkout::not_found",
            f"Board was started in {directory or 'no directory it named'}, which is not in a git checkout;"
            " run the command from inside the checkout to send from",
            {"directory": directory},
        )
    return ran.stdout.strip()


def repository(at: str) -> tuple[str, str]:
    """The owner and repository, in lower case, of the GitHub repository the checkout's origin names.

    This reads the URL as configured, before any `insteadOf` rewrite, so it
    names the repository the person wrote (a soft limit: a rewrite to another
    host still checks as that repository; reading `git remote get-url --push`
    would follow the rewrite).
    """
    ran = subprocess.run(  # noqa: S603 - this reads the checkout's config only
        ["git", "-C", at, "config", "--get", "remote.origin.url"],  # noqa: S607 - git is found on PATH
        capture_output=True,
        text=True,
        timeout=TIMEOUT,
        check=False,
    )
    url = ran.stdout.strip()
    matched = GITHUB.fullmatch(url)
    if matched is None:
        raise PluginError(
            "github::remote::not_github",
            f"the checkout's origin is {url or 'not set'}, which is not a GitHub repository;"
            " set it with `git remote set-url origin https://github.com/<owner>/<repo>.git`",
            {"origin": url},
        )
    return matched[1].lower(), matched[2].lower()


def mount(at: str, root: str) -> str | None:
    """The checkout's path in the personal board, as Board maps it, or None when the personal board does not mount it.

    Board runs in the checkout as the board that mounts it, so `refs .` there
    names the mount. A mount in a board other than the personal one names no
    path a boundary knows, so it counts as none.
    """
    try:
        [here] = Board(at, "github").refs(["."])
        [personal] = Board(root, "github", flags=("--personal",)).refs(["/"])
    except PluginError as error:
        if str(error.fields.get("code", "")).startswith("board::config::"):
            return None
        raise
    if here is None or personal is None:
        return None
    pointers = here["location"]["pointers"]
    if not pointers:
        return None
    board = os.path.realpath(pointers[0]["repository"])
    return str(here["location"]["display"]) if board == os.path.realpath(personal["location"]["repository"]) else None


def branch(at: str) -> str:
    """The branch checked out, or `github::checkout::detached`."""
    name = git(at, "branch", "--show-current").strip()
    if not name:
        raise PluginError(
            "github::checkout::detached",
            "the checkout is on no branch, so there is nothing to push; run `git switch -c <branch>` first",
        )
    return name


def outgoing(at: str) -> list[str]:
    """The commits a push of HEAD would send, oldest first: those no ref on origin reaches.

    A ref on origin that names a commit the checkout lacks cannot reach HEAD,
    so `--ignore-missing` leaves it out.
    """
    advertised = git(at, "ls-remote", "origin").split()
    have = "".join(f"^{sha}\n" for sha in advertised[::2])
    return git(at, "rev-list", "--reverse", "--ignore-missing", "--stdin", "HEAD", stdin=have).split()


def commit_parts(at: str, shas: list[str]) -> dict[str, str]:
    """Each commit's message, author and committer emails and diff, as parts named `commit.<short sha>.<field>`."""
    parts: dict[str, str] = {}
    for sha in shas:
        shown = git(at, "show", "-s", "--format=%ae%x00%ce%x00%B", sha)
        author, committer, message = shown.split("\0", 2)
        prefix = f"commit.{sha[:SHORT]}."
        parts[prefix + "message"] = message.removesuffix("\n")
        parts[prefix + "author_email"] = author
        parts[prefix + "committer_email"] = committer
        parts[prefix + "diff"] = git(at, "show", "--format=", "--diff-merges=first-parent", sha)
    return parts


def push(at: str, name: str, digest: str) -> str:
    """Push the branch to origin and set its upstream: git's porcelain line for the branch, or `github::push::failed`.

    The line, such as `refs/heads/x:refs/heads/x [new branch]`, is what the remote returned for the branch.
    """
    refspec = f"refs/heads/{name}:refs/heads/{name}"
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["git", "-C", at, "push", "--porcelain", "-u", "origin", refspec],  # noqa: S607 - git is found on PATH
            capture_output=True,
            text=True,
            errors="replace",
            timeout=TIMEOUT,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        ran = subprocess.CompletedProcess[str]([], 1, "", f"it did not finish, and may have pushed: {error}")
    lines = [line for line in ran.stdout.splitlines() if f"\t{refspec}\t" in line]
    if ran.returncode != 0 or not lines:
        why = (ran.stderr.strip() or ran.stdout.strip())[-TAIL:]
        raise PluginError(
            "github::push::failed",
            f"`git push origin {name}` failed: {why}; fix what git says, such as pulling first, and run it again",
            {"branch": name, "digest": digest},
        )
    return " ".join(lines[0].split("\t")[1:])
