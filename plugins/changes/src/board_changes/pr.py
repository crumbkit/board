"""The `pr` capability: a pull request body from a change, its plan and its records, and the command to use it."""

import re
import shlex
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_changes.change import (
    CHECK_TYPE,
    OPENED_TYPE,
    PLAN_TYPE,
    PLUGIN,
    RUN_TYPE,
    Change,
    Found,
    read,
    records,
    referenced,
    same_commit,
    write,
)
from board_changes.steps import BRIEF_TYPE, REPORT_TYPE, citing, head_of

HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")

# Severities in the order a report's findings are counted.
SEVERITIES = ("critical", "high", "medium", "low")
# Outcomes in the order a run's results are counted.
OUTCOMES = ("pass", "fail", "error", "skipped")
# Plan sections the body carries, by heading, compared without case.
OUTCOME = "Outcome"
PROBLEM = "Problem"
OUT_OF_SCOPE = "Out of scope"
PROVES = "What the tests prove"
DOES_NOT_PROVE = "What the tests do not prove"
# Cases a run lists as failed, errored or skipped before the rest are only counted.
CASES = 20
# Lines that open or close a Markdown code fence.
FENCES = ("```", "~~~")


def sections(text: str) -> dict[str, str]:
    """The text under each heading of a Markdown document, by heading without case, trimmed.

    A section runs to the next heading of the same or a higher level, so it
    holds its subsections. A line inside a code fence is never a heading.
    """
    found: dict[str, list[str]] = {}
    opened: list[tuple[int, str]] = []
    fenced = False
    for line in text.splitlines():
        match = None if fenced else HEADING.match(line)
        if line.lstrip().startswith(FENCES):
            fenced = not fenced
        level = len(line) - len(line.lstrip("#"))
        while match and opened and opened[-1][0] >= level:
            opened.pop()
        for _, heading in opened:
            found[heading].append(line)
        if match:
            heading = str(match.group(1)).casefold()
            found.setdefault(heading, [])
            opened.append((level, heading))
    return {name: "\n".join(lines).strip() for name, lines in found.items()}


def one_line(value: object) -> str:
    """`value` on one line, so a record's text cannot start a heading or a list item."""
    return " ".join(str(value).split())


def quoted(value: object) -> str:
    """`value` on one line as inline code, with backticks made quotes; `(empty)` when it holds no text."""
    text = one_line(value)
    return "`" + text.replace("`", "'") + "`" if text else "(empty)"


def counted(items: list[dict[str, Any]], key: str, names: tuple[str, ...]) -> str:
    """How many of `items` hold each of `names` at `key`, such as `3 pass, 1 skipped`; `none` when empty."""
    counts = [(sum(1 for item in items if item.get(key) == name), name) for name in names]
    return ", ".join(f"{count} {name}" for count, name in counts if count) or "none"


def run_lines(path: str, run: dict[str, Any], head: str | None, cases: int = CASES) -> tuple[str, list[str]]:
    """What one run proves, as one line, and each limit on it, as lines, naming at most `cases` cases."""
    results: list[dict[str, Any]] = run.get("results") or []
    versions: dict[str, Any] = run.get("versions") or {}
    proves = (
        f"{quoted(path)}: {quoted(run.get('suite'))}, {one_line(run.get('scope'))} run, "
        f"results {counted(results, 'outcome', OUTCOMES)}"
    )
    if run.get("summary"):
        proves += f", summary {quoted(run['summary'])}"
    limits: list[str] = []
    if run.get("scope") != "full":
        limits.append(f"{quoted(path)} ran only targeted cases.")
    if run.get("dirty") is not False:
        limits.append(f"{quoted(path)} does not record a clean tree.")
    if head is not None and not any(same_commit(version, head) for version in versions.values()):
        limits.append(f"{quoted(path)} did not run on the head commit {quoted(head)}.")
    unmet = [result for result in results if result.get("outcome") in ("fail", "error", "skipped")]
    for result in unmet[:cases]:
        note = f": {quoted(result['note'])}" if result.get("note") else ""
        limits.append(f"{quoted(path)} case {quoted(result.get('case'))} is {result['outcome']}{note}")
    if len(unmet) > cases:
        limits.append(f"and {len(unmet) - cases} more cases; see {quoted(path)}")
    return proves, limits


def report_line(path: str, report: dict[str, Any]) -> str:
    """One review report: its verdict, who wrote it, the range it covers and its findings by severity."""
    findings: list[dict[str, Any]] = report.get("findings") or []
    return (
        f"{quoted(path)}: {one_line(report.get('verdict'))} by {quoted(report.get('by'))} over "
        f"{one_line(report.get('range'))}, "
        f"findings {counted(findings, 'severity', SEVERITIES)}"
    )


def bullets(lines: list[str]) -> str:
    """`lines` as a Markdown list."""
    return "\n".join(f"- {line}" for line in lines)


@dataclass(frozen=True)
class Drafted:
    """What the body is drafted from: the change, its newest plan, the runs its checks name and the reviews of it.

    `plan` is the newest plan's text path and `text` that text, both None
    without a plan. `cited` holds each run and report that could be read, by
    logical path as (type, body), and `unreadable` each that could not, with
    the code that says why.
    """

    path: str
    change: Change
    plan: str | None
    text: str | None
    head: str | None
    runs: list[str]
    reports: list[Found]
    cited: dict[str, tuple[str, dict[str, Any]]]
    unreadable: dict[str, str]


def body(drafted: Drafted) -> str:
    """The pull request's body in Markdown.

    The plan's Outcome says what changes and its Problem why. Its "What the
    tests prove" and "What the tests do not prove" sections lead those parts
    of the body, followed by what each check's run shows. Every record read
    is listed as evidence.
    """
    change = drafted.change
    plan = sections(drafted.text or "")
    runs = [(path, drafted.cited[path][1]) for path in drafted.runs if drafted.cited.get(path, ("",))[0] == RUN_TYPE]
    proves = [plan[PROVES.casefold()]] if plan.get(PROVES.casefold()) else []
    limits = [plan[DOES_NOT_PROVE.casefold()]] if plan.get(DOES_NOT_PROVE.casefold()) else []
    shown: list[str] = []
    for path, run in runs:
        line, limited = run_lines(path, run, drafted.head)
        shown.append(line)
        if limited:
            limits.append(bullets(limited))
    if shown:
        proves.append(bullets(shown))
    if not runs:
        limits.append(bullets(["No check of this change names a test run."]))
    missing = "_The plan's {} section is empty._"
    parts = [
        plan.get(OUTCOME.casefold()) or missing.format(OUTCOME),
        "## Why\n\n" + (plan.get(PROBLEM.casefold()) or missing.format(PROBLEM)),
    ]
    if plan.get(OUT_OF_SCOPE.casefold()):
        parts.append("## Out of scope\n\n" + plan[OUT_OF_SCOPE.casefold()])
    parts.append(f"## {PROVES}\n\n" + ("\n\n".join(proves) or missing.format(PROVES)))
    parts.append(f"## {DOES_NOT_PROVE}\n\n" + ("\n\n".join(limits) or missing.format(DOES_NOT_PROVE)))
    if drafted.reports:
        parts.append("## Review\n\n" + bullets([report_line(path, report) for path, report in drafted.reports]))
    parts.append("## Evidence\n\n" + bullets(evidence(drafted)))
    issue = change.get("issue")
    if issue and not any(character.isspace() for character in issue):
        parts.append(f"Closes {issue}")
    return "\n\n".join(parts) + "\n"


def evidence(drafted: Drafted) -> list[str]:
    """The change, its plan, then each run and report, with its type or why it could not be read."""
    lines = [f"{quoted(drafted.path)}: the change"]
    if drafted.plan:
        lines.append(f"{quoted(drafted.plan)}: the plan" + ("" if drafted.text is not None else ", missing"))
    for path in [*drafted.runs, *(path for path, _ in drafted.reports)]:
        found = drafted.cited.get(path)
        cited = found[0] if found else f"unreadable, {drafted.unreadable.get(path)}"
        lines.append(f"{quoted(path)}: {cited}")
    return lines


def draft(board: Board, path: str) -> tuple[Drafted, str | None]:
    """What the body is drafted from, and the pull request URL the change's `opened` record holds, if any."""
    change = read(board, path)
    held = records(board, path)
    newest = held.newest(PLAN_TYPE)
    plan = None if newest is None else str(newest[1]["text"])
    text = None
    if plan is not None:
        [found] = board.get([plan])
        text = None if isinstance(found, PluginError) else found.decode(errors="replace")
    runs = list(dict.fromkeys(str(body["run"]) for _, body in held.of(CHECK_TYPE)))
    briefs = [source for source, _, _ in citing(board, [path], "/for", (BRIEF_TYPE,))]
    reports = (
        [(source, report) for source, _, report in citing(board, briefs, "/brief", (REPORT_TYPE,))] if briefs else []
    )
    cited, unreadable = referenced(board, runs)
    for source, report in reports:
        cited[source] = (REPORT_TYPE, report)
    opened = held.newest(OPENED_TYPE)
    url = None if opened is None else str(opened[1]["pull_request"])
    head = head_of(board, change, held)
    return Drafted(path, change, plan, text, head, runs, reports, cited, unreadable), url


def pr(request: CallRequest) -> dict[str, object]:
    """Write `<change>/pr.md` and return its path and the `gh pr create` line that would use it.

    The line is for a POSIX shell. A change with an `opened` record gets its
    URL instead, and no pr.md is written.
    """
    path = str(request.input["path"]).rstrip("/")
    board = Board(request.board.root, PLUGIN)
    drafted, url = draft(board, path)
    target = f"{path}/pr.md"
    left = list(drafted.unreadable)
    if url is not None:
        return {"path": target, "command": None, "pull_request": url, "unreadable": left}
    write(board, [{"path": target, "text": body(drafted)}])
    [written] = board.refs([target])
    location = cast("dict[str, Any]", written)["location"]
    file = f"{location['repository']}/{location['path']}"
    title = shlex.quote(one_line(drafted.change["title"]))
    command = f"gh pr create --title {title} --body-file {shlex.quote(file)}"
    return {"path": target, "command": command, "pull_request": None, "unreadable": left}
