"""`run`: review the range a brief names with the agent BOARD_REVIEW_AGENT names, and record the report."""

import contextlib
import json
import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_review.git import Repository
from board_review.shapes import (
    AGENT_VARIABLE,
    BRIEF_TYPE,
    RUN_TIMEOUT,
    BriefRecord,
    Cost,
    Finding,
    ReportRecord,
    RunOutput,
)

# Codes that depend on the tree rather than the record. A record holding one is still one to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")
# How much of the agent's stdout or stderr an error carries.
TAIL = 4096
# How long the agent may run: a minute less than the capability's timeout, so the plugin can stop it and report.
AGENT_TIMEOUT = RUN_TIMEOUT - 60
# How long to read the agent's output after its group is killed. A process that left the group may hold it open.
DRAIN = 5
# The signals that stop the plugin, and with it the agent: Board's SIGTERM, Ctrl-C and a closed terminal.
STOPPING = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)

INSTRUCTIONS = """You review a change to a git repository. You have not seen it before.
Read the brief, then the diff. You have no tools and no copy of the repository, so the diff is all you have.

Answer with your review, and end your answer with one JSON object and nothing after it:
{"verdict": "approve" | "changes" | "comment",
 "findings": [{"severity": "high" | "medium" | "low", "file": "<path>", "line": <number>, "text": "<what is wrong>"}]}
Leave out "file" and "line" for a finding about the whole change. Use "changes" when any finding must be fixed.
"""


def run(request: CallRequest) -> RunOutput:
    """Review the brief's range with the agent BOARD_REVIEW_AGENT names, then write the report beside the brief.

    The brief comes only from the board's own tree, and the agent only from
    the user's environment, since a board's tree may come from a pull request.
    The range's base is replaced by the merge base of the base and the head.
    The report is written at /reviews/<id>/report, beside the brief at
    /reviews/<id>/brief, and a review that has one is refused.
    """
    board = Board(request.board.root, "review")
    path = str(request.input["brief"])
    brief = cast("BriefRecord", read_record(board, path, BRIEF_TYPE, "brief")[0])
    report_path = path.removesuffix("brief") + "report"
    if board.rows(f"SELECT 1 FROM entries WHERE logical = {literal(report_path)}"):  # noqa: S608 - literal quoted
        raise PluginError(
            "review::report::exists",
            f"{report_path} already holds a report; write a brief under a new /reviews/<id> whose `follows` is"
            f" {report_path}, and run that",
            {"report": report_path},
        )
    command = _command()
    repository = Repository(request.board.root)
    base_name, _, head_name = brief["range"].partition("..")
    head = repository.commit(head_name)
    # A base that moved on after the branch point would show its own commits as reverted.
    base = repository.merge_base(repository.commit(base_name), head)
    commits = repository.commits(base, head)
    ids = repository.patch_ids(commits)
    prompt = _prompt(path, brief, base, head, repository.diff(base, head))
    clock = time.monotonic()
    # An empty directory, so the agent finds no project settings or MCP servers beside the untrusted diff.
    with tempfile.TemporaryDirectory(prefix="board-review-") as empty:
        stdout = _agent(command, prompt, empty, AGENT_TIMEOUT)
    answer, tokens, models = _envelope(stdout)
    verdict, findings = _answer(answer, stdout)
    cost: Cost = {"seconds": round(time.monotonic() - clock, 3)}
    if tokens:
        cost["input_tokens"], cost["output_tokens"] = tokens["input_tokens"], tokens["output_tokens"]
    report: ReportRecord = {
        "$type": "review/report/v1",
        "brief": path,
        "range": f"{base}..{head}",
        "patch_ids": [ids[commit] for commit in commits if commit in ids],
        "findings": findings,
        "verdict": verdict,
        "by": ", ".join(models) or Path(command[0]).name,
        "cost": cost,
    }
    if "follows" in brief:
        report["follows"] = brief["follows"]
    line = {"path": report_path, "expect": "none", "record": report}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return {"report": report_path, "range": report["range"], "verdict": verdict, "findings": len(findings)}


def _command() -> list[str]:
    """The agent's argv from BOARD_REVIEW_AGENT, a JSON array of strings, or why it cannot be read."""
    text = os.environ.get(AGENT_VARIABLE)
    if text is None:
        raise PluginError(
            "review::agent::not_set",
            f"{AGENT_VARIABLE} is not set; set it to the agent's argv as a JSON array of strings, as README.md shows",
            {"variable": AGENT_VARIABLE},
        )
    try:
        parsed: object = json.loads(text)
    except ValueError:
        parsed = None
    parts = cast("list[object]", parsed) if isinstance(parsed, list) else []
    # An argument may be empty, as in `--tools ""`; the program may not.
    if not parts or not parts[0] or not all(isinstance(part, str) for part in parts):
        raise PluginError(
            "review::agent::invalid",
            f"{AGENT_VARIABLE} is {text[:200]!r}, which is not a JSON array of strings whose first names a program;"
            ' set it to one, such as ["claude", "-p"]',
            {"variable": AGENT_VARIABLE},
        )
    return [str(part) for part in parts]


def read_record(board: Board, path: str, record_type: str, area: str) -> tuple[dict[str, Any], str]:
    """The valid record of `record_type` at `path` in the board's own tree, and its blob.

    Else `review::<area>::not_found`.
    """
    codes = ", ".join(literal(code) for code in TREE_CODES)
    where = f"source = 'worktree' AND logical = {literal(path)}"
    rows = board.rows(
        f"SELECT body, blob FROM records WHERE {where} AND type = {literal(record_type)}"  # noqa: S608 - literals quoted
        f" AND (status = 'ok' OR code IN ({codes}))"
    )
    if not rows:
        raise PluginError(
            f"review::{area}::not_found",
            f"no valid {record_type} is in the board's own tree at {path}; `board get {path}` and `board check`"
            " show what is there, and README.md says what the record holds",
            {"path": path},
        )
    return cast("dict[str, Any]", json.loads(rows[0][0])), str(rows[0][1])


def _prompt(path: str, brief: BriefRecord, base: str, head: str, diff: str) -> str:
    questions = "".join(f"- {question}\n" for question in brief.get("questions", []))
    return (
        INSTRUCTIONS
        + f"\n## Brief {path}\n\nRange: {base}..{head}\n\nFocus: {brief['focus']}\n"
        + (f"\nQuestions:\n{questions}" if questions else "")
        + f"\n## Diff\n\n{diff}"
    )


def _agent(command: list[str], prompt: str, directory: str, timeout: float) -> str:
    """What the agent printed on stdout, given `prompt` on stdin in `directory`.

    The agent leads its own process group. The group is killed when the
    agent exits, after `timeout` seconds, or when the plugin gets one of
    `STOPPING`, so neither the agent nor a process it started outlives the call.
    """
    try:
        agent = subprocess.Popen(  # noqa: S603 - the user's own BOARD_REVIEW_AGENT names the agent
            command,
            cwd=directory,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
    except OSError as error:
        raise PluginError(
            "review::agent::not_started",
            f"the agent {command[0]!r} cannot be started: {error}; change {AGENT_VARIABLE}",
            {"command": command},
        ) from error

    def stop(signum: int, _frame: object) -> None:
        _kill(agent.pid)
        raise SystemExit(128 + signum)

    previous = {signum: signal.signal(signum, stop) for signum in STOPPING}
    try:
        out, err = agent.communicate(prompt.encode(), timeout=timeout)
    except subprocess.TimeoutExpired:
        _kill(agent.pid)
        out = _drain(agent)
        raise PluginError(
            "review::agent::timeout",
            f"the agent {command[0]!r} ran past {timeout:g} seconds and was stopped; narrow the brief's range,"
            f" or make the agent {AGENT_VARIABLE} names answer sooner",
            {"seconds": timeout, "stdout": out.decode(errors="replace")[-TAIL:]},
        ) from None
    finally:
        _kill(agent.pid)
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    stdout = out.decode(errors="replace")
    if agent.returncode != 0:
        stderr = err.decode(errors="replace")[-TAIL:]
        raise PluginError(
            "review::agent::failed",
            f"the agent {command[0]!r} exited {agent.returncode}; its stdout ends: {stdout.strip()[-200:]}; its"
            f" stderr ends: {stderr.strip()[-200:]}",
            {"exit": agent.returncode, "stderr": stderr, "stdout": stdout[-TAIL:]},
        )
    return stdout


def _drain(agent: "subprocess.Popen[bytes]") -> bytes:
    """What the killed agent printed on stdout, read for at most `DRAIN` seconds."""
    try:
        out, _ = agent.communicate(timeout=DRAIN)
    except subprocess.TimeoutExpired:
        for stream in (agent.stdout, agent.stderr):
            if stream is not None:
                stream.close()
        agent.wait()
        return b""
    return out


def _kill(group: int) -> None:
    """Kill every process in the agent's group, which may already have exited.

    macOS refuses with EPERM a group whose processes have all exited but are not yet reaped.
    """
    with contextlib.suppress(ProcessLookupError, PermissionError):
        os.killpg(group, signal.SIGKILL)


def _envelope(stdout: str) -> tuple[str, dict[str, int], list[str]]:
    """The agent's answer, its tokens and models: from `claude -p --output-format json`'s envelope, else none."""
    try:
        parsed: object = json.loads(stdout)
    except ValueError:
        return stdout, {}, []
    if not isinstance(parsed, dict) or not isinstance(cast("dict[str, Any]", parsed).get("result"), str):
        return stdout, {}, []
    envelope = cast("dict[str, Any]", parsed)
    usage = envelope.get("usage")
    tokens: dict[str, int] = {}
    if isinstance(usage, dict):
        counts = {key: value for key, value in cast("dict[str, Any]", usage).items() if isinstance(value, int)}
        inputs = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens")
        tokens = {
            "input_tokens": sum(counts.get(key, 0) for key in inputs),
            "output_tokens": counts.get("output_tokens", 0),
        }
    models = envelope.get("modelUsage")
    names = sorted(cast("dict[str, Any]", models)) if isinstance(models, dict) else []
    return str(envelope["result"]), tokens, names


def _answer(answer: str, stdout: str) -> tuple[Any, list[Finding]]:
    """The verdict and findings of the last JSON object in `answer` that holds a verdict."""
    decoder = json.JSONDecoder()
    start = answer.rfind("{")
    while start != -1:
        try:
            found, _ = decoder.raw_decode(answer, start)
        except ValueError:
            found = None
        if isinstance(found, dict) and "verdict" in found:
            checked = _checked(cast("dict[str, Any]", found))
            if checked is not None:
                return checked
            break
        start = answer.rfind("{", 0, start)
    raise PluginError(
        "review::agent::bad_output",
        "the agent's answer does not end with a JSON object holding a verdict of approve, changes or comment and"
        " findings each with a severity of high, medium or low and a text; change the agent's command or prompt",
        {"stdout": stdout[-TAIL:]},
    )


def _checked(found: dict[str, Any]) -> tuple[Any, list[Finding]] | None:
    """The verdict and findings in `found`, each as the report's schema needs them, or None."""
    verdict, findings = found.get("verdict"), found.get("findings", [])
    if verdict not in ("approve", "changes", "comment") or not isinstance(findings, list):
        return None
    kept: list[Finding] = []
    for item in cast("list[object]", findings):
        if not isinstance(item, dict):
            return None
        item = cast("dict[str, Any]", item)
        severity, text, file, line = item.get("severity"), item.get("text"), item.get("file"), item.get("line")
        if severity not in ("high", "medium", "low") or not isinstance(text, str) or not text:
            return None
        finding: Finding = {"severity": severity, "text": text}
        if isinstance(file, str) and file:
            finding["file"] = file
        if isinstance(line, int) and not isinstance(line, bool) and line >= 1:
            finding["line"] = line
        kept.append(finding)
    return verdict, kept
