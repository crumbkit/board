"""The commits under review, read with git from the repository that holds the board.

The plugin reads the repository's commits and never writes to it: review
records reach the board only through `board put`. Git runs without the
user's or the system's config, and diffs take fixed options, so a patch id
is the same on every machine.
"""

import json
import os
import subprocess
from dataclasses import dataclass
from typing import cast

from board_plugin.protocol import PluginError

from board_review.shapes import BRIEF_TYPE, REPORT_TYPE

# The directory, from the board's root, that holds every review record.
REVIEW_PATH = "reviews/"
# The type of the record each file name holds in /reviews/<id>/.
RECORDS = {"brief.json": BRIEF_TYPE, "report.json": REPORT_TYPE}
# The variables that keep the user's and the system's git config out of every git command.
ISOLATED = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
# Options that fix a diff's text, whatever the repository's own config says.
DIFF = (
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--text",
    "-U3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
)
# The options plan/review.md gives a patch id's diff, with those a repository's config could change fixed.
PATCH = (
    "-p",
    "--full-index",
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "-U3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
)
CONFIG = ("-c", "diff.algorithm=myers", "-c", "diff.noprefix=false")


@dataclass(frozen=True)
class Repository:
    """The git repository whose working tree holds the board at `root`."""

    root: str

    def git(self, *arguments: str, stdin: bytes = b"") -> str:
        """What `git <arguments>` printed, or `review::git::failed` with its stderr."""
        try:
            ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
                ["git", *CONFIG, *arguments],  # noqa: S607 - git is found on PATH
                cwd=self.root,
                input=stdin,
                capture_output=True,
                env=os.environ | ISOLATED,
                check=False,
            )
        except OSError as error:
            raise PluginError("review::git::failed", f"git cannot be run: {error}; put git on PATH") from error
        if ran.returncode != 0:
            stderr = ran.stderr.decode(errors="replace").strip()[-4096:]
            raise PluginError(
                "review::git::failed",
                f"`git {arguments[0]}` failed in {self.root}: {stderr}; check that the board's root is in a git"
                " repository that holds the commits, and fetch them if it does not",
                {"stderr": stderr},
            )
        return ran.stdout.decode(errors="replace")

    def commit(self, revision: str) -> str:
        """The full id of the commit `revision` names, or `review::range::unknown`."""
        try:
            return self.git("rev-parse", "--verify", "--end-of-options", f"{revision}^{{commit}}").strip()
        except PluginError:
            raise PluginError(
                "review::range::unknown",
                f"{revision!r} names no commit in {self.root}; fetch it, or name a commit `git log` shows",
                {"revision": revision},
            ) from None

    def commits(self, base: str, head: str) -> list[str]:
        """Every commit from `base` to `head`, merges among them, oldest first."""
        return self.git("rev-list", "--reverse", "--end-of-options", f"{base}..{head}").split()

    def within(self, commits: str) -> set[str]:
        """Every commit in the range `commits`, or none when git cannot resolve it, as for a commit not fetched."""
        try:
            return set(self.git("rev-list", "--end-of-options", commits).split())
        except PluginError:
            return set()

    def patch_ids(self, commits: list[str]) -> dict[str, str]:
        """The patch id of each of `commits` that has one, by commit.

        Each is `git diff-tree -p --full-index <commit> | git patch-id --verbatim`.
        A merge or an empty commit prints no patch, so it has no patch id, and
        `--verbatim` keeps whitespace, so a change of indentation alone changes the id.
        For a binary file git hashes the full blob ids, so two binary changes to one path differ.
        """
        if not commits:
            return {}
        stdin = "".join(commit + "\n" for commit in commits).encode()
        patches = self.git("diff-tree", "--stdin", *PATCH, stdin=stdin)
        listed = self.git("patch-id", "--verbatim", stdin=patches.encode())
        return {commit: patch_id for patch_id, commit in (line.split() for line in listed.splitlines())}

    def committed(self) -> dict[str, str]:
        """The blob id at HEAD of each file under /reviews/, by its path from the board's root."""
        listed = self.git("ls-tree", "-r", "-z", "HEAD", "--", REVIEW_PATH).split("\0")[:-1]
        return {name: line.split()[2] for line, name in (entry.split("\t", 1) for entry in listed)}

    def merge_base(self, base: str, head: str) -> str:
        """The best common ancestor of `base` and `head`, or `review::range::unrelated` when they have none."""
        try:
            return self.git("merge-base", "--end-of-options", base, head).strip()
        except PluginError:
            raise PluginError(
                "review::range::unrelated",
                f"{base} and {head} share no commit; name the pull request's base commit as --base, and in a"
                " shallow clone fetch the full history first, as with `git fetch --unshallow` or `fetch-depth: 0`",
                {"base": base, "head": head},
            ) from None

    def diff(self, base: str, head: str) -> str:
        """The diff from `base` to `head`."""
        return self.git("diff", *DIFF, "--end-of-options", base, head)

    def review_only(self, commit: str) -> bool:
        """Whether `commit` only adds briefs and reports at their paths under /reviews/, or sets a report's `source`.

        `review.post` sets `source` to the comment's URL, which changes nothing the report covers.
        """
        prefix = self.git("rev-parse", "--show-prefix").strip()
        listed = (
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--no-renames",
            "--name-only",
            "-r",
            "-z",
            "--end-of-options",
        )
        changed = self.git(*listed, commit).split("\0")[:-1]
        added = self.git(*listed[:-1], "--diff-filter=A", "--end-of-options", commit).split("\0")[:-1]
        return bool(changed) and all(
            self._record(commit, prefix, path) and (path in added or self._sourced(commit, path)) for path in changed
        )

    def _sourced(self, commit: str, path: str) -> bool:
        """Whether `commit` changes only the `source` of the report at `path`."""
        try:
            before, after = (json.loads(self.git("cat-file", "blob", f"{at}:{path}")) for at in (f"{commit}^", commit))
        except (PluginError, ValueError):
            return False
        unset = {"source": None}
        return path.endswith("/report.json") and isinstance(before, dict) and before | unset == after | unset

    def _record(self, commit: str, prefix: str, path: str) -> bool:
        """Whether `path` in `commit` is a brief or a report at /reviews/<id>/ of the type its name gives."""
        inside = prefix + REVIEW_PATH
        parts = path.removeprefix(inside).split("/") if path.startswith(inside) else []
        if len(parts) != 2 or not parts[0] or parts[1] not in RECORDS:  # noqa: PLR2004 - an id and a file name
            return False
        try:
            record: object = json.loads(self.git("cat-file", "blob", f"{commit}:{path}"))
        except (PluginError, ValueError):
            return False
        return isinstance(record, dict) and cast("dict[str, object]", record).get("$type") == RECORDS[parts[1]]
