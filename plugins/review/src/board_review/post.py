"""`post`: comment a report on a pull request with gh, once the owner approves the exact comment."""

import json
import os
import re
import subprocess
from typing import cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_review.git import Repository
from board_review.shapes import POST_TIMEOUT, PostOutput, ReportRecord

# How long gh may run: less than the capability's timeout, so the plugin can report a stopped gh.
GH_TIMEOUT = POST_TIMEOUT - 20
# Keeps gh from waiting on a prompt or checking for its own updates.
GH_QUIET = {"GH_PROMPT_DISABLED": "1", "GH_NO_UPDATE_NOTIFIER": "1"}
# How much of gh's stderr a failure's message carries.
TAIL = 1000


def post(request: CallRequest) -> PostOutput:
    """Ask the owner with the whole comment, then post it with `gh pr comment` and set the report's `source`.

    A report whose `source` is set has been posted, or came from GitHub, so it
    is skipped without asking. The comment names the pull request, so an
    approval admits only that text on that pull request.
    """
    board = Board(request.board.root, "review")
    path, pr = str(request.input["report"]), str(request.input["pr"])
    [held] = board.get([path])
    if isinstance(held, PluginError):
        raise held
    report = cast("ReportRecord", json.loads(held))
    if "source" in report:
        return {"report": path, "pull_request": pr, "comment": report["source"], "posted": False}
    text = comment(path, pr, report)
    decided = board.document("decide", "--op", "review.post", "--path", path, "--text", "-", stdin=text.encode())
    url = _gh(pr, text)
    blob = Repository(request.board.root).git("hash-object", "--stdin", stdin=held).strip()
    line = {"path": path, "expect": blob, "record": report | {"source": url}}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    output: PostOutput = {"report": path, "pull_request": pr, "comment": url, "posted": True}
    effect = decided.get("data", {}).get("effect")
    if isinstance(effect, str):
        output["effect"] = effect
    return output


def comment(path: str, pr: str, report: ReportRecord) -> str:
    """The comment for `report`: verdict, author, range, findings and the report it follows, then its target.

    An agent wrote the report from an untrusted diff, so every @ is followed by
    a zero-width joiner and mentions no one.
    """
    lines = [f"**Review: {report['verdict']}** by {report['by']}", "", f"Commits `{report['range']}`.", ""]
    for finding in report["findings"]:
        where = finding.get("file", "") + (f":{finding['line']}" if "line" in finding else "")
        lines.append(f"- **{finding['severity']}** " + (_code(where) + " " if where else "") + finding["text"])
    lines += [] if report["findings"] else ["No findings."]
    lines += ["", f"Follows {report['follows']}."] if "follows" in report else []
    lines += ["", "---", f"Report {path}, for {pr}.", ""]
    return "\n".join(lines).replace("@", "@\u200d")


def _code(text: str) -> str:
    """`text` as a Markdown code span whose fence is longer than any run of backticks in it."""
    longest = max((len(run) for run in re.findall("`+", text)), default=0)
    fence = "`" * (longest + 1)
    return f"{fence} {text} {fence}" if longest else f"`{text}`"


def _gh(pr: str, text: str) -> str:
    """The URL `gh pr comment` prints after posting `text` on `pr`, or `review::gh::failed`.

    Board sets an error's `stderr` field to the plugin's own, so gh's goes in the message.
    """
    command = ["gh", "pr", "comment", pr, "--body-file", "-"]
    try:
        ran = subprocess.run(  # noqa: S603 - the arguments are the plugin's own
            command,
            input=text.encode(),
            capture_output=True,
            env=os.environ | GH_QUIET,
            timeout=GH_TIMEOUT,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        ran, why = None, f"did not finish: {error}"
    else:
        why = f"exited {ran.returncode}: {ran.stderr.decode(errors='replace').strip()[-TAIL:]}"
    if ran is not None and ran.returncode == 0:
        return ran.stdout.decode(errors="replace").strip()
    raise PluginError(
        "review::gh::failed",
        f"`gh pr comment {pr}` {why}; check the pull request, gh on PATH and `gh auth status`, then post again",
        {"pull_request": pr, "exit": ran.returncode if ran else None},
    )
