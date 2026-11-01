"""The git pre-push hook: installing it in a checkout, and the check it runs on what git pushes."""

from pathlib import Path

from board_plugin.protocol import CallRequest, PluginError

from board_github import checkout, crossing

# The hook git runs before each push. It refuses the push when board is missing or the check fails or cannot run.
# A personal board that no config names holds no boundary, so a push then goes unchecked.
HOOK = """#!/bin/sh
# Installed by `board call github.install-hook`. Before git pushes, it runs
# `board --personal call github.pre-push`, which has the boundary check what
# the push sends, and refuses the push when the check fails or cannot run.
refs=$(cat)
if ! command -v board >/dev/null 2>&1; then
  echo "board is not on PATH, so the boundary cannot check this push. Next: put board on PATH and push again." >&2
  exit 1
fi
out=$(board --personal call github.pre-push --remote "$1" --url "$2" --refs "$refs" 2>&1) && exit 0
case $out in
  *"code:  board::config::not_found"*) exit 0 ;;
esac
printf '%s\\n' "$out" >&2
exit 1
"""
# The SHA git gives a ref it deletes, which sends no commit.
DELETED = "0" * 40
# The fields of each line git writes on the hook's stdin.
FIELDS = 4


def install_hook(_: CallRequest) -> dict[str, object]:
    """Install the pre-push hook in the checkout Board was started in, refusing to replace a different hook."""
    top = checkout.top()
    hooks = Path(top, checkout.git(top, "rev-parse", "--git-path", "hooks").strip())
    path = hooks / "pre-push"
    if path.exists() or path.is_symlink():
        if path.is_file() and path.read_text(errors="replace") == HOOK:
            return {"path": str(path), "installed": False}
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
    deleted on the remote sends nothing. The parts are those `github.push`
    builds for the same branch and commits, so a push it makes is checked
    against the crossing it has just written and judged verdicts come from the
    cache.
    """
    given = request.input
    top, remote = checkout.top(), str(given["remote"])
    destinations = checkout.destinations(top, remote, str(given["url"]))
    pushed = [line.split() for line in str(given["refs"]).splitlines() if line.strip()]
    pushed = [fields for fields in pushed if len(fields) == FIELDS and fields[1] != DELETED]
    if not destinations or not pushed:
        return {"checked": []}
    origin = checkout.mount(top, request.board.root)
    checked: list[dict[str, object]] = []
    for _, sha, ref, _ in pushed:
        _, parts = checkout.push_parts(top, ref.removeprefix("refs/heads/"), sha, remote)
        for to in destinations:
            passed = crossing.check(request.board.root, "github.push", to, origin, parts)
            checked.append({"ref": ref, "to": to, "digest": passed["digest"], "note": passed["note"]})
    return {"checked": checked}
