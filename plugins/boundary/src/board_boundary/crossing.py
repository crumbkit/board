"""`check`, `verify` and `record`: content checked against boundaries, and its crossing in the personal board."""

import hashlib
import json
from fnmatch import fnmatchcase
from functools import partial
from typing import cast

from board_plugin.board import TREE_CODES, Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_boundary.shapes import (
    BOUNDARIES,
    BOUNDARY_TYPE,
    CROSSING_TYPE,
    CROSSINGS,
    BoundaryRecord,
    CheckOutput,
    CrossingRecord,
    Failure,
    Finding,
    RecordOutput,
    Verdict,
    VerifyOutput,
)

# The finding of a control without a rule, which cannot check content, so its verdict fails the crossing.
# Why an entry is not a boundary, and the step that fixes it.
NOT_A_BOUNDARY = "not a boundary"
MOVE_OUT = "move each entry that is not a boundary out of /boundaries/"
NOT_A_DIRECTORY = "is not a directory"
MAKE_DIRECTORY = "make /boundaries/ a directory, not a symlink or a file"
GIVE_RULE = "give each control without a rule a rule, or remove it from the boundary"
NO_RULE = "this control has no rule, so it cannot check content; give it a rule, or remove it from the boundary"
# The failures a refusal's message names; its `failures` field holds them all.
NAMED = 20
# A valid record: one Board marks ok, or degraded only for a reference that dangles or reaches the wrong type.
VALID = f"(status = 'ok' OR code IN ({', '.join(map(literal, TREE_CODES))}))"
# The op the owner approves to let one failed crossing through, and how Board refuses a decide it has not admitted.
OVERRIDE = "boundary.override"
ASKED = "board::settings::asked"
REFUSED = "board::settings::refused"
TOO_LARGE = "board::json::too_large"
ADMITTED = "admitted"


def personal(request: CallRequest) -> Board:
    """The personal board, which holds the boundaries, their controls and the crossings, whichever board called."""
    return Board(request.board.root, "boundary", flags=("--personal",))


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
    board = personal(request)
    action, to = str(request.input["action"]), str(request.input["to"])
    origin = cast("str | None", request.input.get("from"))
    parts = cast("dict[str, str]", request.input["parts"])
    matched = {path: record for path, record in _boundaries(board).items() if _matches(record, action, to, origin)}
    verdicts, unchecked = _evaluate(board, matched, parts)
    failed = [verdict for verdict in verdicts if verdict["status"] in {"failed", "stated"}]
    key = digest(action, to, parts)
    path = CROSSINGS + key
    # Only a failure asks the owner, so a passing crossing writes no effect.
    answer, effect = _override(board, path, action, to, parts) if failed else (None, None)
    approved = answer == ADMITTED
    passed = not failed or approved
    crossing = CrossingRecord(
        {
            "$type": CROSSING_TYPE,
            "digest": key,
            "action": action,
            "to": to,
            "passed": passed,
            "boundaries": sorted(matched),
            "verdicts": verdicts,
        }
    )
    if origin is not None:
        crossing["from"] = origin
    if approved:
        crossing["override"] = cast("str", effect)
    line: dict[str, object] = {"path": path, "record": crossing}
    prior = _crossing(board, key)
    if prior is not None:
        line["expect"] = prior[1]
        if passed and "result" in prior[0]:
            crossing["result"] = prior[0]["result"]
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    if not passed:
        raise _overridable(_refusal(action, key, path, failed), cast("str", answer), effect)
    note = None
    if not matched:
        note = f"no boundary matches {action} to {to}" + (f" from {origin}" if origin else "") + "; nothing was checked"
    elif not verdicts:
        note = "the boundaries that match name no control for these parts; nothing was checked"
    return CheckOutput(
        digest=key,
        crossing=path,
        boundaries=sorted(matched),
        verdicts=verdicts,
        unchecked=unchecked,
        note=note,
        override=effect if approved else None,
    )


def _evaluate(
    board: Board, matched: dict[str, BoundaryRecord], parts: dict[str, str]
) -> tuple[list[Verdict], list[str]]:
    """Every verdict of the matched boundaries' controls on the parts, and the parts no check names."""
    groups: dict[tuple[str, ...], dict[str, str]] = {}
    unchecked: list[str] = []
    for name, text in sorted(parts.items()):
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
        else:
            unchecked.append(name)
    verdicts: list[Verdict] = []
    for controls, texts in groups.items():
        stdin = json.dumps({"controls": list(controls), "parts": texts}).encode()
        report = board.document("call", "controls.evaluate", stdin=stdin)
        verdicts += cast("list[Verdict]", report["data"]["output"]["results"])
    verdicts.sort(key=lambda verdict: (verdict["control"], verdict["part"]))
    return verdicts, unchecked


def shown(action: str, to: str, parts: dict[str, str]) -> str:
    """The content as the owner's approval shows it: names quoted as JSON, and every body line indented.

    No body line starts in the first column, and a quoted name holds no line
    break, so no two contents show the same text.
    """
    quoted = partial(json.dumps, ensure_ascii=False)
    lines = [f"action {quoted(action)}", f"to {quoted(to)}"]
    for name, body in sorted(parts.items()):
        lines += ["", f"part {quoted(name)}", *(f"  {line}" for line in body.split("\n"))]
    return "\n".join(lines) + "\n"


def _override(board: Board, path: str, action: str, to: str, parts: dict[str, str]) -> tuple[str, str | None]:
    """How the owner answered letting the failed crossing at `path` through, and the effect that asks or approved it.

    The answer is ADMITTED, ASKED, REFUSED or TOO_LARGE, which has no effect.

    `board decide` shows the owner the content, so the signed approval names
    what it lets through, and the path's digest binds the approval to that
    content. Board refuses to show more than 1 MiB, so such content cannot be
    overridden (a soft limit; showing only the parts the checks failed on
    would raise it). Under `board call` Board always names the plugin, so an
    admitted decide names the effect that approved it.
    """
    text = shown(action, to, parts)
    try:
        report = board.document("decide", "--op", OVERRIDE, "--path", path, "--text", "-", stdin=text.encode())
    except PluginError as error:
        code = error.fields.get("code")
        if code in {ASKED, REFUSED}:
            return str(code), str(error.fields["effect"])
        if code == TOO_LARGE:
            return TOO_LARGE, None
        raise
    return ADMITTED, cast("str | None", report["data"]["effect"])


def _boundaries(board: Board) -> dict[str, BoundaryRecord]:
    """Every boundary in the personal board's own tree, by its path, or `boundary::boundary::invalid`.

    Boundaries belong under /boundaries/, but a valid boundary elsewhere is
    read too. Any entry under /boundaries/ that is not a valid boundary,
    a boundary elsewhere that fails its schema, and a /boundaries that is not
    a directory, such as a symlink, refuse every check, so a broken or
    misplaced rule stops content instead of letting it pass unchecked.
    """
    folder = BOUNDARIES.rstrip("/")
    rows = board.rows(
        f"SELECT path, COALESCE(logical, '/' || path), type, {VALID}, code,"  # noqa: S608 - values are quoted
        " (SELECT body FROM records r WHERE r.source = e.source AND r.path = e.path)"
        f" FROM entries e WHERE source = 'worktree' AND (path GLOB {literal(BOUNDARIES + '*')}"
        f" OR path = {literal(folder)} OR type = {literal(BOUNDARY_TYPE)}) ORDER BY path"
    )
    invalid: dict[str, str] = {}
    for path, logical, kind, valid, code, _ in rows:
        if path == folder:
            invalid[logical] = NOT_A_DIRECTORY
        elif kind != BOUNDARY_TYPE:
            invalid[logical] = code or NOT_A_BOUNDARY
        elif not valid:
            invalid[logical] = code
    if invalid:
        raise _invalid(invalid)
    return {logical: json.loads(body) for _, logical, _, _, _, body in rows}


def _invalid(invalid: dict[str, str]) -> PluginError:
    """`boundary::boundary::invalid`, naming each entry and why, with the step that fixes each kind."""
    named = ", ".join(f"{logical} ({reason})" for logical, reason in invalid.items())
    reasons = set(invalid.values())
    steps = [MOVE_OUT] if NOT_A_BOUNDARY in reasons else []
    steps += [MAKE_DIRECTORY] if NOT_A_DIRECTORY in reasons else []
    broken = [logical for logical, reason in invalid.items() if reason not in {NOT_A_BOUNDARY, NOT_A_DIRECTORY}]
    if broken:
        steps.append(f"run `board --personal check {broken[0]}` and fix each record that is not valid")
    return PluginError(
        "boundary::boundary::invalid",
        f"the personal board holds boundaries that are not valid, so nothing can be checked: {named};"
        f" {'; '.join(steps)}; then check again",
        {"boundaries": invalid},
    )


def _matches(record: BoundaryRecord, action: str, to: str, origin: str | None) -> bool:
    """Whether the boundary covers the action to the destination from the checkout.

    Destinations match without case, as GitHub names do. A boundary limited by
    `from` also applies when the caller names no checkout, so a caller that
    could not resolve its checkout is still checked. `from` is compared as
    path text, at or under each path, so a checkout named through a link to
    its mount is not matched (a soft limit; `board refs` resolving both paths
    would match it).
    """
    if not fnmatchcase(to.lower(), record["to"].lower()):
        return False
    if not any(fnmatchcase(action, glob) for glob in record["actions"]):
        return False
    if "from" not in record or origin is None:
        return True
    return any(origin == path or origin.startswith(path.rstrip("/") + "/") for path in record["from"])


def _overridable(refusal: PluginError, answer: str, effect: str | None) -> PluginError:
    """`refusal`, ending with what the owner may do about overriding it, given Board's `answer` and the `effect`."""
    if answer == TOO_LARGE:
        owner = " This content is over 1 MiB as the owner would see it, so the owner cannot approve it; shorten it."
    elif answer == REFUSED:
        owner = (
            f" The owner refused to let this exact content through ({effect});"
            f" the owner can ask again by deleting /effects/seen/{effect} in the personal board."
        )
    else:
        owner = (
            f" The owner may approve this exact content instead with `board answer {effect} approve`; then check again."
        )
    recovery = cast("dict[str, str]", refusal.fields["recovery"])
    fields = refusal.fields | {"recovery": recovery | {"text": recovery["text"] + owner}}
    if effect is not None:
        fields["effect"] = effect
    return PluginError(refusal.code, refusal.message + owner, fields)


def _refusal(action: str, key: str, path: str, failed: list[Verdict]) -> PluginError:
    """`boundary::check::failed`, naming each failing control, part and line, with the next step."""
    failures: list[Failure] = []
    for verdict in failed:
        for finding in verdict["findings"] or [Finding(text=NO_RULE)]:
            failure = Failure(control=verdict["control"], part=verdict["part"], text=finding["text"])
            if "line" in finding:
                failure["line"] = finding["line"]
            if "rewrite" in finding:
                failure["rewrite"] = finding["rewrite"]
            failures.append(failure)
    named = "; ".join(_named(failure) for failure in failures[:NAMED])
    more = f"; and {len(failures) - NAMED} more" if len(failures) > NAMED else ""
    parts = ", ".join(sorted({verdict["part"] for verdict in failed if verdict["status"] == "failed"}))
    steps: list[str] = []
    if any(verdict["status"] == "stated" for verdict in failed):
        steps.append(GIVE_RULE)
    if parts:
        steps.append(
            f"change {parts}: remove or reword each quoted match, shorten each part over its budget,"
            " and follow each rewrite"
        )
    recovery = (
        f"Next: {'; '.join(steps)}. Then run `board call {action}` again, or"
        " `board --personal call boundary.check` with the changed parts on stdin."
    )
    return PluginError(
        "boundary::check::failed",
        f"{len(failures)} finding(s) stop this content: {named}{more}. {recovery}",
        {"digest": key, "crossing": path, "failures": failures, "recovery": {"kind": "hint", "text": recovery}},
    )


def _named(failure: Failure) -> str:
    """One failure as the message names it: control, part, line, the quoted text and any rewrite."""
    line = f" line {failure['line']}" if "line" in failure else ""
    rewrite = f" ({failure['rewrite']})" if "rewrite" in failure else ""
    return f"{failure['control']} on {failure['part']}{line}: {json.dumps(failure['text'])}{rewrite}"


def _crossing(board: Board, key: str) -> tuple[CrossingRecord, str] | None:
    """The valid crossing of `key` in the personal board, and its blob, or None."""
    rows = board.rows(
        "SELECT body, blob FROM records"  # noqa: S608 - values are quoted by literal()
        f" WHERE type = {literal(CROSSING_TYPE)} AND source = 'worktree' AND {VALID}"
        f" AND logical = {literal(CROSSINGS + key)}"
    )
    return (cast("CrossingRecord", json.loads(rows[0][0])), str(rows[0][1])) if rows else None


def verify(request: CallRequest) -> VerifyOutput:
    """Whether a passing crossing exists for the digest given, or for the content given."""
    given = request.input
    content = [name for name in ("action", "to", "parts") if name in given]
    if ("digest" in given) == bool(content) or 0 < len(content) < 3:  # noqa: PLR2004 - all three or none
        then = (
            f"run `board --personal call boundary.verify --digest {given['digest']}`"
            if "digest" in given
            else "pass all of action, to and parts on stdin, or only --digest"
        )
        raise PluginError(
            "boundary::verify::bad_input",
            "verify takes a digest, or the action, to and parts of the content, and not both;"
            f" it was given {', '.join(sorted(given)) or 'nothing'}; {then}",
            {"given": sorted(given)},
        )
    key = (
        str(given["digest"])
        if "digest" in given
        else digest(str(given["action"]), str(given["to"]), cast("dict[str, str]", given["parts"]))
    )
    board = personal(request)
    found = _crossing(board, key)
    if found is None:
        return VerifyOutput(digest=key, crossing=None, passed=False, result=None)
    record = found[0]
    # The crossing passes only while the same boundaries match. This misses an edit to a matched boundary's
    # checks or to a control's rule, until the crossing records the blob of each record it was checked against.
    matching = sorted(
        path
        for path, boundary in _boundaries(board).items()
        if _matches(boundary, record["action"], record["to"], record.get("from"))
    )
    passed = record["passed"] and matching == record["boundaries"]
    return VerifyOutput(digest=key, crossing=CROSSINGS + key, passed=passed, result=record.get("result"))


def record(request: CallRequest) -> RecordOutput:
    """Set the result of the passing crossing of the digest, or refuse naming why there is none."""
    board = personal(request)
    key, result = str(request.input["digest"]), str(request.input["result"])
    path = CROSSINGS + key
    found = _crossing(board, key)
    if found is None:
        raise PluginError(
            "boundary::crossing::not_found",
            f"no crossing is at {path} in the personal board;"
            " run `board --personal call boundary.check` on the content first",
            {"crossing": path},
        )
    crossing, blob = found
    if not crossing["passed"]:
        raise PluginError(
            "boundary::crossing::failed",
            f"the crossing at {path} failed its checks, so nothing was sent to record;"
            " change the content and run `board --personal call boundary.check` again",
            {"crossing": path},
        )
    line = {"path": path, "expect": blob, "record": crossing | {"result": result}}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return RecordOutput(crossing=path, result=result)
