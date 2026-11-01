"""The caller's clone: the git repository Board was started in, read with git and never written."""

from board_plugin.protocol import PluginError
from board_plugin.repos import caller_origin, caller_root, git

# The branches taken as the default when the clone records no origin/HEAD.
DEFAULTS = ("main", "master")


def root() -> str:
    """The root of the caller's clone, or `changes::clone::none` when Board was not started in one."""
    found = caller_root()
    if found is None:
        raise PluginError(
            "changes::clone::none",
            "Board was not started in a git clone, so there is no worktree to record; run the command from the"
            " clone of the repository the change is to",
        )
    return found


def remote() -> str:
    """The origin of the caller's clone, or `changes::clone::no_remote` naming why there is none."""
    origin, why = caller_origin()
    if origin is None:
        raise PluginError(
            "changes::clone::no_remote",
            f"{why or 'Board was not started in a git clone'}, so the change has no repository; run start from a"
            " clone whose origin names the repository",
        )
    return origin


def branch(directory: str) -> str:
    """The work branch checked out at `directory`.

    A detached HEAD is `changes::branch::detached`, and the default branch,
    origin/HEAD or else main or master, is `changes::branch::default`, so a
    change always names its own branch.
    """
    found = git(directory, "symbolic-ref", "--quiet", "--short", "HEAD")
    if not found:
        raise PluginError(
            "changes::branch::detached",
            f"the clone at {directory} has no branch checked out; check out a work branch with"
            " `git switch -c <branch>`, then start the change",
            {"clone": directory},
        )
    origin_head = git(directory, "symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD")
    defaults = (origin_head.removeprefix("origin/"),) if origin_head else DEFAULTS
    if found in defaults:
        raise PluginError(
            "changes::branch::default",
            f"the clone at {directory} is on {found}, the default branch; check out a work branch with"
            " `git switch -c <branch>`, then start the change",
            {"clone": directory, "branch": found},
        )
    return found
