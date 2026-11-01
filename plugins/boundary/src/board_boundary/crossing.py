"""`check`, `verify` and `record`: content checked against boundaries, and its crossing in the personal board."""

import hashlib
import json
import subprocess
from fnmatch import fnmatchcase
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_boundary.shapes import (
    BOUNDARY_TYPE,
    CROSSING_TYPE,
    CROSSINGS,
    BoundaryRecord,
    CheckOutput,
    CrossingRecord,
    Finding,
    RecordOutput,
    Verdict,
    VerifyOutput,
)

# The failures a refusal's message names; its `failures` field holds them all.
NAMED = 20
# Board marks a valid record degraded for a reference that dangles or reaches the wrong type.
VALID = "(status = 'ok' OR code IN ('board::ref::dangling', 'board::ref::wrong_type'))"


class Personal(Board):
    """The personal board, which holds the boundaries, their controls and the crossings, whichever board called."""

    def _run(
        self, *arguments: str, stdin: bytes = b"", timeout: float | None = None
    ) -> subprocess.CompletedProcess[bytes]:
        return super()._run("--personal", *arguments, stdin=stdin, timeout=timeout)


def digest(action: str, to: str, parts: dict[str, str]) -> str:
    """The SHA-256 of the canonical JSON of the action, destination and parts: keys sorted, no spaces, UTF-8."""
    content = {"action": action, "parts": parts, "to": to}
    return hashlib.sha256(
        json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    ).hexdigest()


def check(request: CallRequest) -> CheckOutput:
    """Evaluate the parts against each matching boundary's controls, write the crossing, and refuse on a failure.

    Parts are grouped by the controls that apply to them, and each group is
    one `controls.evaluate`, so a part's budget and pattern rules run before
    its judged rules. Groups run one after another within the one timeout.
    """
    board = Personal(request.board.root, "boundary")
    action, to = str(request.input["action"]), str(request.input["to"])
    origin = cast("str | None", request.input.get("from"))
    parts = cast("dict[str, str]", request.input["parts"])
    matched = {path: record for path, record in _boundaries(board).items() if _matches(record, action, to, origin)}
    groups: dict[tuple[str, ...], dict[str, str]] = {}
    for name, text in parts.items():
        controls = sorted(
            {
                check["control"]
                for record in matched.values()
                for check in record["checks"]
                if any(fnmatchcase(name, glob) for glob in check["parts"])
            }
        )
        if controls:
            groups.setdefault(tuple(controls), {})[name] = text
    verdicts: list[Verdict] = []
    for controls, texts in groups.items():
        stdin = json.dumps({"controls": list(controls), "parts": texts}).encode()
        report = board.document("call", "controls.evaluate", stdin=stdin)
        verdicts += cast("list[Verdict]", report["data"]["output"]["results"])
    verdicts.sort(key=lambda verdict: (verdict["control"], verdict["part"]))
    failed = [verdict for verdict in verdicts if verdict["status"] == "failed"]
    key = digest(action, to, parts)
    crossing = CrossingRecord(
        {
            "$type": CROSSING_TYPE,
            "digest": key,
            "action": action,
            "to": to,
            "passed": not failed,
            "boundaries": sorted(matched),
            "verdicts": verdicts,
        }
    )
    if origin is not None:
        crossing["from"] = origin
    path = CROSSINGS + key
    board.document("put", "--batch", stdin=(json.dumps({"path": path, "record": crossing}) + "\n").encode())
    if failed:
        raise _refusal(action, key, path, failed)
    note = None
    if not matched:
        note = f"no boundary matches {action} to {to}" + (f" from {origin}" if origin else "") + "; nothing was checked"
    elif not verdicts:
        note = "the boundaries that match name no control for these parts; nothing was checked"
    return CheckOutput(digest=key, crossing=path, boundaries=sorted(matched), verdicts=verdicts, note=note)


def _boundaries(board: Board) -> dict[str, BoundaryRecord]:
    """Every valid boundary in the personal board's own tree, by its path."""
    rows = board.rows(
        "SELECT logical, body FROM records"  # noqa: S608 - the values are constants
        f" WHERE type = {literal(BOUNDARY_TYPE)} AND source = 'worktree' AND {VALID} ORDER BY logical"
    )
    return {logical: json.loads(body) for logical, body in rows}


def _matches(record: BoundaryRecord, action: str, to: str, origin: str | None) -> bool:
    """Whether the boundary covers the action to the destination from the checkout.

    `from` is compared as path text, at or under each path, so a checkout
    named through a link to its mount is not matched (a soft limit; `board refs`
    resolving both paths would match it).
    """
    if not fnmatchcase(to, record["to"]) or not any(fnmatchcase(action, glob) for glob in record["actions"]):
        return False
    if "from" not in record:
        return True
    return origin is not None and any(
        origin == path or origin.startswith(path.rstrip("/") + "/") for path in record["from"]
    )


def _refusal(action: str, key: str, path: str, failed: list[Verdict]) -> PluginError:
    """`boundary::check::failed`, naming each failing control, part and line, with the next step."""
    failures: list[dict[str, Any]] = []
    for verdict in failed:
        for finding in verdict["findings"] or [Finding(text="failed")]:
            failure: dict[str, Any] = {"control": verdict["control"], "part": verdict["part"], "text": finding["text"]}
            if "line" in finding:
                failure["line"] = finding["line"]
            if "rewrite" in finding:
                failure["rewrite"] = finding["rewrite"]
            failures.append(failure)
    named = "; ".join(_named(failure) for failure in failures[:NAMED])
    more = f"; and {len(failures) - NAMED} more" if len(failures) > NAMED else ""
    parts = ", ".join(sorted({verdict["part"] for verdict in failed}))
    recovery = (
        f"Next: change {parts}: remove or reword each quoted match, shorten each part over its budget,"
        f" and follow each rewrite. Then run `board call {action}` again, which checks the changed parts."
    )
    return PluginError(
        "boundary::check::failed",
        f"{len(failures)} finding(s) stop this content: {named}{more}. {recovery}",
        {"digest": key, "crossing": path, "failures": failures, "recovery": {"kind": "hint", "text": recovery}},
    )


def _named(failure: dict[str, Any]) -> str:
    """One failure as the message names it: control, part, line, the quoted text and any rewrite."""
    line = f" line {failure['line']}" if "line" in failure else ""
    rewrite = f" ({failure['rewrite']})" if "rewrite" in failure else ""
    return f"{failure['control']} on {failure['part']}{line}: {json.dumps(failure['text'])}{rewrite}"


def _crossing(board: Board, key: str) -> tuple[CrossingRecord, str] | None:
    """The valid crossing of `key` in the personal board, and its blob, or None."""
    rows = board.rows(
        "SELECT body, blob FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CROSSING_TYPE)} AND source = 'worktree' AND status = 'ok'"
        f" AND logical = {literal(CROSSINGS + key)}"
    )
    return (cast("CrossingRecord", json.loads(rows[0][0])), str(rows[0][1])) if rows else None


def verify(request: CallRequest) -> VerifyOutput:
    """Whether a passing crossing exists for the digest given, or for the content given."""
    given = request.input
    content = [name for name in ("action", "to", "parts") if name in given]
    if ("digest" in given) == bool(content) or 0 < len(content) < 3:  # noqa: PLR2004 - all three or none
        raise PluginError(
            "boundary::verify::bad_input",
            "verify takes a digest, or the action, to and parts of the content, and not both;"
            f" it was given {', '.join(sorted(given)) or 'nothing'};"
            " run `board call boundary.verify --digest <digest>`",
            {"given": sorted(given)},
        )
    key = (
        str(given["digest"])
        if "digest" in given
        else digest(str(given["action"]), str(given["to"]), cast("dict[str, str]", given["parts"]))
    )
    found = _crossing(Personal(request.board.root, "boundary"), key)
    if found is None:
        return VerifyOutput(digest=key, crossing=None, passed=False, result=None)
    record = found[0]
    return VerifyOutput(digest=key, crossing=CROSSINGS + key, passed=record["passed"], result=record.get("result"))


def record(request: CallRequest) -> RecordOutput:
    """Set the result of the passing crossing of the digest, or refuse naming why there is none."""
    board = Personal(request.board.root, "boundary")
    key, result = str(request.input["digest"]), str(request.input["result"])
    path = CROSSINGS + key
    found = _crossing(board, key)
    if found is None:
        raise PluginError(
            "boundary::crossing::not_found",
            f"no crossing is at {path} in the personal board; run `board call boundary.check` on the content first",
            {"crossing": path},
        )
    crossing, blob = found
    if not crossing["passed"]:
        raise PluginError(
            "boundary::crossing::failed",
            f"the crossing at {path} failed its checks, so nothing was sent to record;"
            " change the content and run `board call boundary.check` again",
            {"crossing": path},
        )
    line = {"path": path, "expect": blob, "record": crossing | {"result": result}}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return RecordOutput(crossing=path, result=result)
