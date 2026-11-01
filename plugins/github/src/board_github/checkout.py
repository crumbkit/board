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
# The user and password of a URL with a scheme, which a message never quotes.
USERINFO = re.compile(r"://[^/@\s]+@")
# The short SHA that names a commit's parts, at a fixed length so a commit's part names never change.
SHORT = 12
# Variables only the boundary's judged rules need, kept from git, its hooks and gh.
MODEL_VARIABLES = ("ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN")


def without_model_keys() -> dict[str, str]:
    """The plugin's environment without the model credentials, for git and gh."""
    return {key: value for key, value in os.environ.items() if key not in MODEL_VARIABLES}


def shown(text: str) -> str:
    """`text` with the user and password removed from every URL in it."""
    return USERINFO.sub("://", text)


def run(top: str, *arguments: str, stdin: str = "") -> subprocess.CompletedProcess[str]:
    """The finished `git <arguments>` in `top`; one that did not finish exits 1 saying why."""
    try:
        return subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            ["git", "-C", top, *arguments],  # noqa: S607 - git is found on PATH
            input=stdin,
            capture_output=True,
            text=True,
            errors="replace",
            timeout=TIMEOUT,
            check=False,
            env=without_model_keys(),
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return subprocess.CompletedProcess[str]([], 1, "", f"it did not finish: {error}")


def git(top: str, *arguments: str, stdin: str = "") -> str:
    """What `git <arguments>` prints in `top`, or `github::git::failed` naming the command and git's error."""
    ran = run(top, *arguments, stdin=stdin)
    if ran.returncode != 0:
        command = " ".join(["git", *arguments[:2]])
        raise PluginError(
            "github::git::failed",
            f"`{command}` exited {ran.returncode}: {shown(ran.stderr.strip()[-TAIL:])}; check git and the checkout",
            {"command": command},
        )
    return ran.stdout


def top() -> str:
    """The root of the git checkout Board was started in, which Board names in BOARD_CALLER_DIR."""
    directory = os.environ.get("BOARD_CALLER_DIR", "")
    ran = run(directory or ".", "rev-parse", "--show-toplevel")
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
    host still checks as that repository). `insteadOf` moves fetch and push
    alike, so the commits checked are still those the push sends; a push URL
    that differs from the fetch URL is `github::remote::split`.
    """
    url = run(at, "config", "--get", "remote.origin.url").stdout.strip()
    matched = GITHUB.fullmatch(url)
    if matched is None:
        raise PluginError(
            "github::remote::not_github",
            f"the checkout's origin is {shown(url) or 'not set'}, which is not a GitHub repository;"
            " set it with `git remote set-url origin https://github.com/<owner>/<repo>.git`",
            {"origin": shown(url)},
        )
    fetch = git(at, "remote", "get-url", "origin").strip()
    pushes = git(at, "remote", "get-url", "--push", "--all", "origin").split()
    if pushes != [fetch]:
        raise PluginError(
            "github::remote::split",
            f"origin fetches from {shown(fetch)} but pushes to {shown(', '.join(pushes))}, so the commits checked"
            " would not be those sent; push from a checkout whose origin fetches and pushes to the same repository,"
            " such as after `git config --unset-all remote.origin.pushurl`",
            {"fetch": shown(fetch), "push": [shown(each) for each in pushes]},
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


def branch(at: str) -> tuple[str, str]:
    """The branch checked out and the commit it is at, resolved once, or `github::checkout::detached`."""
    name = git(at, "branch", "--show-current").strip()
    if not name:
        raise PluginError(
            "github::checkout::detached",
            "the checkout is on no branch, so there is nothing to push; run `git switch -c <branch>` first",
        )
    return name, git(at, "rev-parse", "--verify", "HEAD^{commit}").strip()


def outgoing(at: str, head: str) -> list[str]:
    """The commits a push of `head` would send, oldest first: those no ref on origin reaches.

    A ref on origin that names a commit the checkout lacks cannot reach `head`,
    so `--ignore-missing` leaves it out.
    """
    advertised = git(at, "ls-remote", "origin").split()
    have = "".join(f"^{sha}\n" for sha in advertised[::2])
    return git(at, "rev-list", "--reverse", "--ignore-missing", "--stdin", head, stdin=have).split()


def commit_parts(at: str, shas: list[str]) -> dict[str, str]:
    """Each commit's message, names, emails, diff and binary paths, as parts named `commit.<short sha>.<field>`.

    The diff is git's text of every file, binary or not, with no textconv or
    external diff, so it holds the bytes sent; `binary` lists, one per line,
    the paths git would otherwise show only as "Binary files differ".
    """
    parts: dict[str, str] = {}
    for sha in shas:
        fields = git(at, "show", "-s", "--format=%an%x00%ae%x00%cn%x00%ce%x00%B", sha).split("\0", 4)
        prefix = f"commit.{sha[:SHORT]}."
        for name, value in zip(
            ("author_name", "author_email", "committer_name", "committer_email"), fields, strict=False
        ):
            parts[prefix + name] = value
        parts[prefix + "message"] = fields[4].removesuffix("\n")
        diff = ("--format=", "--diff-merges=first-parent", "--no-color", "--no-ext-diff", "--no-textconv")
        parts[prefix + "diff"] = git(at, "show", *diff, "--text", sha)
        counts = git(at, "show", *diff, "--numstat", "-z", sha)
        parts[prefix + "binary"] = "".join(f"{path}\n" for path in _binary(counts))
    return parts


def _binary(numstat: str) -> list[str]:
    """The paths `--numstat -z` counts as binary, with `-` for lines added and removed; a rename gives its target."""
    paths: list[str] = []
    fields = iter(numstat.split("\0"))
    for field in fields:
        added, _, rest = field.lstrip("\n").partition("\t")
        removed, _, path = rest.partition("\t")
        if not path and removed:
            next(fields, "")
            path = next(fields, "")
        if (added, removed) == ("-", "-"):
            paths.append(path)
    return paths


def push(at: str, name: str, head: str, digest: str) -> str:
    """Push `head` to the branch on origin and set its upstream: git's porcelain line, or `github::push::failed`.

    Only the checked commit goes: no tags, no submodule commits and no push
    options, whatever git's config says. The line, such as
    `<sha>:refs/heads/x [new branch]`, is what the remote returned.
    """
    refspec = f"{head}:refs/heads/{name}"
    ran = run(
        at,
        "-c",
        "push.pushOption=",
        "push",
        "--porcelain",
        "--no-follow-tags",
        "--recurse-submodules=no",
        "origin",
        refspec,
    )
    lines = [line for line in ran.stdout.splitlines() if f"\t{refspec}\t" in line]
    if ran.returncode != 0 or not lines:
        why = shown((ran.stderr.strip() or ran.stdout.strip())[-TAIL:])
        raise PluginError(
            "github::push::failed",
            f"`git push origin {name}` failed, and may have pushed if it did not finish: {why};"
            " fix what git says, such as pulling first, and run it again",
            {"branch": name, "digest": digest},
        )
    git(at, "branch", f"--set-upstream-to=origin/{name}", name)
    return " ".join(lines[0].split("\t")[1:])
