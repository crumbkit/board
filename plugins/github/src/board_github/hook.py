"""The git pre-push hook: installing it in a checkout, and the check it runs on what git pushes."""

import os
from pathlib import Path

from board_plugin.protocol import CallRequest, PluginError

from board_github import checkout, crossing

# The line that marks a hook as this plugin's, so a later install replaces an older copy and no other hook.
MARKER = "# Installed by `board call github.install-hook`."
# The variable that carries the URL git pushes to, kept off board's command line, where `ps` would show it.
PUSH_URL = "BOARD_GITHUB_PUSH_URL"
# The hook git runs before each push. It refuses the push when board is missing or the check fails or cannot run.
# It lets the push go only when the code field of Board's failure document says no config names a personal board.
# The probe's output holds no content of the push, so no quoted text can match it. The sed reads the first `code`
# field before any nested object, which is the document's own; JSON escapes a quote inside any string.
HOOK = f"""#!/bin/sh
{MARKER}
# Before git pushes, it runs `board --personal call github.pre-push`, which
# has the boundary check what the push sends, and refuses the push when the
# check fails or cannot run.
refs=$(cat)
if ! command -v board >/dev/null 2>&1; then
  echo "board is not on PATH, so the boundary cannot check this push. Next: put board on PATH and push again." >&2
  exit 1
fi
code=$(board --json --personal refs / 2>/dev/null |
  sed -n 's/^{{[^{{}}]*"code"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/p')
[ "$code" = board::config::not_found ] && exit 0
{PUSH_URL}=$2 board --personal call github.pre-push --remote "$1" --refs "$refs" >/dev/null || exit 1
"""
# The fields of each line git writes on the hook's stdin.
FIELDS = 4
# The config scopes whose `core.hooksPath` serves repositories other than this one.
SHARED = {"global", "system"}


def install_hook(_: CallRequest) -> dict[str, object]:
    """Install the pre-push hook in the checkout Board was started in, refusing to replace a different hook."""
    top = checkout.top()
    scope = checkout.run(top, "config", "--show-scope", "--get", "core.hooksPath").stdout.split("\t")[0]
    hooks = Path(top, checkout.git(top, "rev-parse", "--git-path", "hooks").strip())
    if scope in SHARED:
        raise PluginError(
            "github::hook::shared",
            f"core.hooksPath in the {scope} git config names {hooks}, which serves every repository, so the hook is"
            " not installed; set core.hooksPath for this repository with `git config core.hooksPath .git/hooks`,"
            " then install again",
            {"path": str(hooks), "scope": scope},
        )
    path = hooks / "pre-push"
    if path.exists() or path.is_symlink():
        text = path.read_text(errors="replace") if path.is_file() else ""
        if text == HOOK:
            return {"path": str(path), "installed": False}
        if MARKER not in text.splitlines():
            raise PluginError(
                "github::hook::exists",
                f"{path} already holds a pre-push hook that is not this plugin's, and it is left as it was;"
                " move it aside and run `board call github.install-hook` again",
                {"path": str(path)},
            )
    hooks.mkdir(parents=True, exist_ok=True)
    path.write_text(HOOK)
    path.chmod(0o755)
    return {"path": str(path), "installed": True}


def pre_push(request: CallRequest) -> dict[str, object]:
    """Check what git pushes, as `github.push` would check it, for each ref git names on stdin.

    Each line is `<local ref> <local sha> <remote ref> <remote sha>`. A ref
    deleted on the remote, whose SHA is all zeros, sends nothing. The commits
    are those no ref at the URL git pushes to reaches, so a push URL unlike the
    fetch URL is compared with what it holds. The parts are those `github.push`
    builds for the same branch and commits, so judged verdicts on a push it
    makes come from the cache.
    """
    given = request.input
    top, remote = checkout.top(), str(given["remote"])
    url = os.environ.get(PUSH_URL, "")
    destinations = checkout.destinations(top, remote, url)
    pushed = [line.split() for line in str(given["refs"]).splitlines() if line.strip()]
    pushed = [fields for fields in pushed if len(fields) == FIELDS and set(fields[1]) != {"0"}]
    if not destinations or not pushed:
        return {"checked": []}
    origin = checkout.mount(top, request.board.root)
    # One `ls-remote` serves every ref, so `git push --tags` makes one round trip.
    have = checkout.advertised(top, url or remote)
    checked: list[dict[str, object]] = []
    for _, sha, ref, _ in pushed:
        parts = checkout.ref_parts(top, ref, sha, have)
        for to in destinations:
            passed = crossing.check(request.board.root, "github.push", to, origin, parts)
            checked.append({"ref": ref, "to": to, "digest": passed["digest"], "note": passed["note"]})
    return {"checked": checked}


def covers(request: CallRequest) -> dict[str, object]:
    """The destinations of a write from the checkout that a boundary covers, and the boundaries, checking nothing.

    The Claude Code mod asks this before a raw `git push` or `gh` write runs.
    The destination is the repository `repo` names, else the URLs of `remote`
    as the pre-push hook reads them. With no remote, a push goes to the
    branch's push remote, else origin, and a gh write to origin. The checkout's
    mount and the matching rules are the sending actions' own.
    """
    given = request.input
    top = checkout.top()
    if "repo" in given:
        destinations = [f"github:{str(given['repo']).lower()}"]
    else:
        remote = str(given.get("remote", ""))
        if not remote and given["action"] == "push":
            branch = checkout.run(top, "symbolic-ref", "-q", "HEAD").stdout.strip()
            remote = checkout.run(top, "for-each-ref", "--format=%(push:remotename)", branch).stdout.strip()
        destinations = checkout.destinations(top, remote or "origin", "", push=given["action"] == "push")
    origin = checkout.mount(top, request.board.root)
    covered: list[dict[str, object]] = []
    for to in destinations:
        boundaries = crossing.covers(request.board.root, f"github.{given['action']}", to, origin)
        if boundaries:
            covered.append({"to": to, "boundaries": boundaries})
    return {"covered": covered}
