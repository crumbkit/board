"""The `pr` capability: a pull request body from a change, its plan and its records, and the command to use it."""

import shlex
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest

from board_changes.change import PLUGIN, read, write
from board_changes.gates import REPORT_TYPE, RUN_TYPE, Observed, of_type, same_commit, sections
from board_changes.phases import plan_text, referenced

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


def body(observed: Observed) -> str:
    """The pull request's body in Markdown.

    The plan's Outcome says what changes and its Problem why. Its "What the
    tests prove" and "What the tests do not prove" sections lead those parts
    of the body, followed by what each attached run shows. Every reference is
    listed as evidence.
    """
    change = observed.change
    plan = sections(observed.plan or "")
    head = change.get("head")
    runs = of_type(observed, RUN_TYPE)
    reports = of_type(observed, REPORT_TYPE)
    proves = [plan[PROVES.casefold()]] if plan.get(PROVES.casefold()) else []
    limits = [plan[DOES_NOT_PROVE.casefold()]] if plan.get(DOES_NOT_PROVE.casefold()) else []
    shown: list[str] = []
    for path, run in runs:
        line, limited = run_lines(path, run, head)
        shown.append(line)
        if limited:
            limits.append(bullets(limited))
    if shown:
        proves.append(bullets(shown))
    if not runs:
        limits.append(bullets(["No test run is attached to this change."]))
    missing = "_The plan's {} section is empty._"
    parts = [
        plan.get(OUTCOME.casefold()) or missing.format(OUTCOME),
        "## Why\n\n" + (plan.get(PROBLEM.casefold()) or missing.format(PROBLEM)),
    ]
    if plan.get(OUT_OF_SCOPE.casefold()):
        parts.append("## Out of scope\n\n" + plan[OUT_OF_SCOPE.casefold()])
    parts.append(f"## {PROVES}\n\n" + ("\n\n".join(proves) or missing.format(PROVES)))
    parts.append(f"## {DOES_NOT_PROVE}\n\n" + ("\n\n".join(limits) or missing.format(DOES_NOT_PROVE)))
    if reports:
        parts.append("## Review\n\n" + bullets([report_line(path, report) for path, report in reports]))
    parts.append("## Evidence\n\n" + bullets(evidence(observed)))
    issue = change.get("issue")
    if issue and not any(character.isspace() for character in issue):
        parts.append(f"Closes {issue}")
    return "\n\n".join(parts) + "\n"


def evidence(observed: Observed) -> list[str]:
    """Each reference, in the change's order, with its type or why it could not be read, after the change and plan."""
    lines = [f"{quoted(observed.path)}: the change"]
    plan = observed.change.get("plan")
    if plan:
        lines.append(f"{quoted(plan)}: the plan" + ("" if observed.plan is not None else ", missing"))
    for path in observed.change.get("records", []):
        found = observed.records.get(path)
        cited = found[0] if found else f"unreadable, {observed.unreadable.get(path)}"
        lines.append(f"{quoted(path)}: {cited}")
    return lines


def pr(request: CallRequest) -> dict[str, object]:
    """Write `<change>/pr.md` and return its path and the `gh pr create` line that would use it.

    The line is for a POSIX shell. A change that records a pull request gets
    its URL instead, and no pr.md is written.
    """
    path = str(request.input["path"]).rstrip("/")
    board = Board(request.board.root, PLUGIN)
    change, _ = read(board, path)
    records, unreadable = referenced(board, change.get("records", []))
    observed = Observed(path, change, records, plan_text(board, change), None, unreadable)
    target = f"{path}/pr.md"
    left = list(unreadable)
    opened = change.get("pull_request")
    if opened is not None:
        return {"path": target, "command": None, "pull_request": opened, "unreadable": left}
    write(board, [{"path": target, "text": body(observed)}])
    [written] = board.refs([target])
    location = cast("dict[str, Any]", written)["location"]
    file = f"{location['repository']}/{location['path']}"
    title = shlex.quote(one_line(change["title"]))
    command = f"gh pr create --title {title} --body-file {shlex.quote(file)}"
    return {"path": target, "command": command, "pull_request": None, "unreadable": left}
