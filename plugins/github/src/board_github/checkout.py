"""The checkout Board was started in: its GitHub repository, its mount, its branch, and what a push would send."""

import os
import re
import subprocess
from collections.abc import Callable

from board_plugin.board import Board
from board_plugin.protocol import PluginError

# How long one git command may run, in seconds. A push or ls-remote reaches the network.
TIMEOUT = 300
# How much of git's stderr a failure's message carries.
TAIL = 2000
# The hosts that serve GitHub's git, and the `github.com-<alias>` ssh hosts kept for several accounts.
GITHUB_HOSTS = re.compile(r"(?:www\.|ssh\.)?github\.com|github\.com-[\w.-]+")
# A URL with a scheme, as host and path, any user and port left out.
SCHEMED = re.compile(r"([A-Za-z][A-Za-z0-9+.-]*)://(?:[^@/]*@)?(\[[^\]/]*\]|[^:/]*)(?::\d*)?(/.*)?")
# Git's scp-like ssh form, `[user@]host:path`, whose host holds no slash.
SCP = re.compile(r"(?:[^@/:]*@)?([^@/:]+):(.*)")
# A GitHub repository's path: owner and repository, with an optional `.git` and trailing slash.
REPOSITORY = re.compile(r"/?([A-Za-z0-9-]+)/([A-Za-z0-9._-]+?)(?:\.git)?/?")
# The schemes git reaches a host over ssh by.
SSH = {"ssh", "git+ssh", "ssh+git"}
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


def ssh_hostname(host: str) -> str | None:
    """The host ssh connects to for `host`, from `ssh -G`, which reads ssh's config and connects to nothing."""
    try:
        ran = subprocess.run(  # noqa: S603 - the host is passed after `--`
            ["ssh", "-G", "--", host],  # noqa: S607 - ssh is found on PATH
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
            env=without_model_keys(),
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    names = [line.split(None, 1)[1] for line in ran.stdout.splitlines() if line.startswith("hostname ")]
    return names[0].strip().lower() if ran.returncode == 0 and names else None


def github_repository(url: str, resolve: Callable[[str], str | None] = ssh_hostname) -> tuple[str, str] | None:
    """The owner and repository, in lower case, of the GitHub repository `url` reaches, or None for another place.

    Every sending action and the pre-push hook read a URL by this one rule.
    A local path, a `file://` URL and a host other than GitHub's are None. An
    ssh host that is not GitHub's by name is resolved through ssh's config, so
    a `Host` alias of github.com counts. A URL that could reach GitHub but
    names no repository this rule reads, such as an alias ssh cannot resolve
    or a `<transport>::<address>` remote helper, is `github::remote::unknown`.
    """
    schemed = SCHEMED.fullmatch(url)
    scp = None if schemed or "::" in url or url.startswith(("/", ".", "~")) else SCP.fullmatch(url)
    found: tuple[str, str, str] | None = None
    if schemed is not None:
        found = (schemed[1].lower(), schemed[2].lower(), schemed[3] or "")
    elif scp is not None:
        found = ("ssh", scp[1].lower(), scp[2])
    elif "::" not in url:
        return None
    if found is not None and found[0] == "file":
        return None
    if found is not None and found[0] in SSH and not GITHUB_HOSTS.fullmatch(found[1]):
        resolved = None if found[1].startswith("-") else resolve(found[1])
        found = None if resolved is None else (found[0], resolved, found[2])
    if found is not None and not GITHUB_HOSTS.fullmatch(found[1]):
        return None
    matched = None if found is None else REPOSITORY.fullmatch(found[2])
    if matched is None:
        raise PluginError(
            "github::remote::unknown",
            f"{shown(url)} could reach GitHub, but it names no repository the plugin can read, so what it sends"
            " cannot be checked; use a URL of the form https://github.com/<owner>/<repo>.git or"
            " git@github.com:<owner>/<repo>.git",
            {"url": shown(url)},
        )
    return matched[1].lower(), matched[2].lower()


def repository(at: str) -> tuple[str, str]:
    """The owner and repository, in lower case, of the GitHub repository the checkout's origin names.

    This reads the URL as configured, before any `insteadOf` rewrite, so it
    names the repository the person wrote (a soft limit: a rewrite to another
    host still checks as that repository). `insteadOf` moves fetch and push
    alike, so the commits checked are still those the push sends; a push URL
    that differs from the fetch URL is `github::remote::split`.
    """
    url = run(at, "config", "--get", "remote.origin.url").stdout.strip()
    matched = github_repository(url)
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
    return matched


def destinations(at: str, remote: str, url: str) -> list[str]:
    """The GitHub destinations, `github:<owner>/<repo>` in lower case, of a push git makes to `remote` at `url`.

    Git names the remote, or the URL it was given in its place, and the URL it
    pushes to, after any rewrite. Each of them is read, and so are the
    remote's push URLs as configured, else its URLs, as `repository` reads
    origin's. A URL that could reach GitHub but cannot be read is
    `github::remote::unknown`, so the hook fails closed. None naming GitHub
    means no boundary on GitHub covers the push.
    """
    configured = run(at, "config", "--get-all", f"remote.{remote}.pushurl").stdout.split()
    configured = configured or run(at, "config", "--get-all", f"remote.{remote}.url").stdout.split()
    named = [url, *configured] if configured else [url, remote]
    found = (github_repository(each) for each in named if each)
    return sorted({f"github:{owner}/{repo}" for owner, repo in (each for each in found if each is not None)})


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


def outgoing(at: str, head: str, remote: str = "origin") -> list[str]:
    """The commits a push of `head` to `remote` would send, oldest first: those no ref on the remote reaches.

    A ref on origin that names a commit the checkout lacks cannot reach `head`,
    so `--ignore-missing` leaves it out.
    """
    advertised = git(at, "ls-remote", remote).split()
    have = "".join(f"^{sha}\n" for sha in advertised[::2])
    return git(at, "rev-list", "--reverse", "--ignore-missing", "--stdin", head, stdin=have).split()


def push_parts(at: str, branch: str, head: str, remote: str = "origin") -> tuple[list[str], dict[str, str]]:
    """The commits a push of `head` to `branch` on `remote` sends, and the parts that name the branch and each commit.

    `github.push` and the pre-push hook both build their parts here, so a push
    the action makes gives the hook the same content to check.
    """
    commits = outgoing(at, head, remote)
    return commits, {"push.branch": branch} | commit_parts(at, commits)


# A tag object's tagger line: name, email, then the time and zone.
TAGGER = re.compile(r"tagger (.*) <([^>]*)> \d+ [+-]\d{4}")


def ref_parts(at: str, name: str, sha: str, remote: str) -> dict[str, str]:
    """The parts of a push of object `sha` to the ref `name` on `remote`: the tags it peels through, then the commits.

    An annotated tag's message, tagger name and tagger email are parts named
    `tag.<short sha>.<field>`, for each tag down to the commit. A ref whose
    object is a tree or a blob is `github::push::not_commit`.
    """
    parts: dict[str, str] = {}
    kind = git(at, "cat-file", "-t", sha).strip()
    while kind == "tag":
        headers, _, message = git(at, "cat-file", "tag", sha).partition("\n\n")
        lines = headers.splitlines()
        tagger = next((found for line in lines if (found := TAGGER.fullmatch(line))), None)
        prefix = f"tag.{sha[:SHORT]}."
        parts[prefix + "message"] = message.removesuffix("\n")
        parts[prefix + "tagger_name"] = tagger[1] if tagger else ""
        parts[prefix + "tagger_email"] = tagger[2] if tagger else ""
        sha = next(line.split()[1] for line in lines if line.startswith("object "))
        kind = git(at, "cat-file", "-t", sha).strip()
    if kind != "commit":
        raise PluginError(
            "github::push::not_commit",
            f"{name} names a {kind}, which the boundary cannot check as commits; push a branch or a tag of a commit",
            {"ref": name, "object": sha},
        )
    return push_parts(at, name.removeprefix("refs/heads/"), sha, remote)[1] | parts


def commit_parts(at: str, shas: list[str]) -> dict[str, str]:
    """Each commit's message, names, emails, diff and binary paths, as parts named `commit.<short sha>.<field>`.

    The diff is git's text of every file, binary or not, with no textconv or
    external diff and with file names unquoted, so it holds the bytes sent;
    `binary` lists, one per line, the paths git would otherwise show only as
    "Binary files differ". `headers` holds the commit's headers other than
    tree, parents, author and committer, such as a merged tag's `mergetag`.
    A commit that changes a file Git LFS stores is `github::push::lfs`.
    """
    parts: dict[str, str] = {}
    for sha in shas:
        diff = ("--format=", "--diff-merges=first-parent", "--no-color", "--no-ext-diff", "--no-textconv")
        changed = _changed(git(at, "-c", "core.quotePath=false", "show", *diff, "--numstat", "-z", sha))
        _refuse_lfs(at, sha, [path for path, _ in changed])
        fields = git(at, "show", "-s", "--format=%an%x00%ae%x00%cn%x00%ce%x00%B", sha).split("\0", 4)
        prefix = f"commit.{sha[:SHORT]}."
        for name, value in zip(
            ("author_name", "author_email", "committer_name", "committer_email"), fields, strict=False
        ):
            parts[prefix + name] = value
        parts[prefix + "message"] = fields[4].removesuffix("\n")
        parts[prefix + "headers"] = _headers(git(at, "cat-file", "commit", sha))
        parts[prefix + "diff"] = git(at, "-c", "core.quotePath=false", "show", *diff, "--text", sha)
        parts[prefix + "binary"] = "".join(f"{path}\n" for path, binary in changed if binary)
    return parts


def _changed(numstat: str) -> list[tuple[str, bool]]:
    """Each path `--numstat -z` names, and whether it counts as binary (`-` for lines added and removed).

    A rename gives its target.
    """
    paths: list[tuple[str, bool]] = []
    fields = iter(numstat.split("\0"))
    for field in fields:
        added, _, rest = field.lstrip("\n").partition("\t")
        removed, _, path = rest.partition("\t")
        if not path and removed:
            next(fields, "")
            path = next(fields, "")
        if path:
            paths.append((path, (added, removed) == ("-", "-")))
    return paths


def _headers(commit: str) -> str:
    """The commit object's header lines other than tree, parent, author and committer, with their continuations."""
    kept: list[str] = []
    keep = False
    for line in commit.partition("\n\n")[0].splitlines():
        if not line.startswith(" "):
            keep = line.split(" ", 1)[0] not in {"tree", "parent", "author", "committer"}
        if keep:
            kept.append(line + "\n")
    return "".join(kept)


def _refuse_lfs(at: str, sha: str, paths: list[str]) -> None:
    """`github::push::lfs` when any of the commit's paths is stored by Git LFS, whose hook uploads unchecked bytes."""
    if not paths:
        return
    answer = git(at, "check-attr", "-z", f"--source={sha}", "--stdin", "filter", stdin="\0".join(paths) + "\0")
    fields = answer.split("\0")
    stored = [fields[i] for i in range(0, len(fields) - 2, 3) if fields[i + 2] == "lfs"]
    if stored:
        raise PluginError(
            "github::push::lfs",
            f"commit {sha[:SHORT]} changes {', '.join(stored)}, which Git LFS stores; its hook uploads those files'"
            " bytes where the boundary cannot check them, so nothing was pushed; stop tracking them with"
            " `git lfs untrack` and commit them as plain files, or ask the owner how to send them",
            {"commit": sha, "paths": stored},
        )


def push(at: str, name: str, head: str, digest: str) -> str:
    """Push `head` to the branch on origin: git's porcelain line, or `github::push::failed`.

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
    return " ".join(lines[0].split("\t")[1:])


def track(at: str, name: str) -> None:
    """Make origin's branch the branch's upstream as `git push -u` does, which needs no remote-tracking ref."""
    git(at, "config", f"branch.{name}.remote", "origin")
    git(at, "config", f"branch.{name}.merge", f"refs/heads/{name}")
