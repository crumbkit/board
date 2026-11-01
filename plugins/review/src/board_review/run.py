"""`run`: review the range or record a brief names with the agent BOARD_REVIEW_AGENT names, and record the report."""

import hashlib
import json
import time
from pathlib import Path
from typing import Any, cast

from board_plugin import agent
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
    Subject,
)

# Codes that depend on the tree rather than the record. A record holding one is still one to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")
# How long the agent may run: a minute less than the capability's timeout, so the plugin can stop it and report.
AGENT_TIMEOUT = RUN_TIMEOUT - 60

INSTRUCTIONS = """You review {what}. You have not seen it before.
Read the brief, then the {shown}. You have no tools and no copy of the repository, so the {shown} is all you have.

Answer with your review, and end your answer with one JSON object and nothing after it:
{"verdict": "approve" | "changes" | "comment",
 "findings": [{"severity": "high" | "medium" | "low", "file": "<path>", "line": <number>, "text": "<what is wrong>"}]}
Leave out "file" and "line" for a finding about the whole {shown}. Use "changes" when any finding must be fixed.
"""


def run(request: CallRequest) -> RunOutput:
    """Review the brief's range or subject with the agent BOARD_REVIEW_AGENT names, then write the report beside it.

    The brief comes only from the board's own tree, and the agent only from
    the user's environment, since a board's tree may come from a pull request.
    A range's base is replaced by the merge base of the base and the head. A
    subject is read through Board, and must still be at its pinned blob.
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
    command = agent.command("review", AGENT_VARIABLE)
    subject = brief.get("subject")
    covered: tuple[str, list[str]] | None = None
    if subject is not None:
        prompt = _prompt(path, brief, "a record in a board", "record", _record(board, subject))
    else:
        repository = Repository(request.board.root)
        # The schema gives a brief without a subject a range.
        base_name, _, head_name = cast("dict[str, str]", brief)["range"].partition("..")
        head = repository.commit(head_name)
        # A base that moved on after the branch point would show its own commits as reverted.
        base = repository.merge_base(repository.commit(base_name), head)
        commits = repository.commits(base, head)
        ids = repository.patch_ids(commits)
        shown = f"Range: {base}..{head}\n\n## Diff\n\n{repository.diff(base, head)}"
        prompt = _prompt(path, brief, "a change to a git repository", "diff", shown)
        covered = (f"{base}..{head}", [ids[commit] for commit in commits if commit in ids])
    clock = time.monotonic()
    stdout = agent.ask(command, prompt, AGENT_TIMEOUT, "narrow the brief's range")
    answer, tokens, models = agent.envelope(stdout)
    verdict, findings = _answer(answer, stdout)
    cost: Cost = {"seconds": round(time.monotonic() - clock, 3)}
    if tokens:
        cost["input_tokens"], cost["output_tokens"] = tokens["input_tokens"], tokens["output_tokens"]
    report: ReportRecord = {
        "$type": "review/report/v1",
        "brief": path,
        "findings": findings,
        "verdict": verdict,
        "by": ", ".join(models) or Path(command.argv[0]).name,
        "cost": cost,
    }
    output: RunOutput = {"report": report_path, "verdict": verdict, "findings": len(findings)}
    if subject is not None:
        report["subject"] = output["subject"] = subject
    if covered is not None:
        report["range"], report["patch_ids"] = covered
        output["range"] = covered[0]
    if "follows" in brief:
        report["follows"] = brief["follows"]
    line = {"path": report_path, "expect": "none", "record": report}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return output


def _record(board: Board, subject: Subject) -> str:
    """The prompt's part for the subject: its path, blob, the references it makes and its bytes.

    Else `review::subject::not_found`, or `review::subject::moved` when the record is no longer at its pinned blob.
    The bytes are read once and hashed as git hashes a blob, so the agent reads exactly the pinned version.
    """
    record = subject["record"]
    [answer] = board.refs([record])
    if answer is None:
        raise PluginError(
            "review::subject::not_found",
            f"no file is at the subject {record}; `board ls` shows what is there, so fix the brief's subject",
            {"record": record},
        )
    [read] = board.get([record])
    if isinstance(read, PluginError):
        raise read
    # SHA-256 repositories are unsupported: Board's gix is built with only its sha1 feature and cannot open one.
    current = hashlib.sha1(b"blob %d\0%b" % (len(read), read), usedforsecurity=False).hexdigest()
    if current != subject["blob"]:
        raise PluginError(
            "review::subject::moved",
            f"the subject {record} is at blob {current}, not the {subject['blob']} the brief pins; write a new brief"
            f" that pins {current} if that version is the one to review",
            {"record": record, "pinned": subject["blob"], "current": current},
        )
    references = "".join(f"- {ref['pointer']}: {ref['target']}\n" for ref in answer["from"])
    return (
        f"Record: {record} at {current}\n\nReferences it makes:\n{references or '- none\n'}"
        f"\n## Record\n\n{read.decode(errors='replace')}"
    )


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


def _prompt(path: str, brief: BriefRecord, what: str, shown: str, body: str) -> str:
    """The agent's prompt: the instructions for reviewing `what` from its `shown`, the brief, then `body`."""
    questions = "".join(f"- {question}\n" for question in brief.get("questions", []))
    return (
        INSTRUCTIONS.replace("{what}", what).replace("{shown}", shown)
        + f"\n## Brief {path}\n\nFocus: {brief['focus']}\n"
        + (f"\nQuestions:\n{questions}" if questions else "")
        + f"\n{body}"
    )


def _answer(answer: str, stdout: str) -> tuple[Any, list[Finding]]:
    """The verdict and findings of the last JSON object in `answer` that holds a verdict."""
    found = agent.last_object(answer, "verdict")
    checked = None if found is None else _checked(found)
    if checked is not None:
        return checked
    raise PluginError(
        "review::agent::bad_output",
        "the agent's answer does not end with a JSON object holding a verdict of approve, changes or comment and"
        " findings each with a severity of high, medium or low and a text; change the agent's command or prompt",
        {"stdout": stdout[-agent.TAIL :]},
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
