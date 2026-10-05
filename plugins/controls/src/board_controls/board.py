"""Reading the board through the real `board`: queries, references and file bytes."""

import json
import subprocess
from typing import Any, cast

from pydantic import ValidationError

from board_controls.models import CONTROL_TYPE, ControlRecord
from board_controls.protocol import PluginError

# The most rows one query returns. A query that is cut short would make an
# answer wrong, so it is an error.
LIMIT = 10_000


def _run(root: str, *arguments: str) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(  # noqa: S603 - the program is Board itself, found on PATH
            ["board", *arguments],  # noqa: S607
            cwd=root,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        raise PluginError("controls::board::failed", f"board cannot be run: {error}") from error


def _failed(
    arguments: tuple[str, ...], ran: subprocess.CompletedProcess[bytes], why: str, code: object = None
) -> PluginError:
    return PluginError(
        "controls::board::failed",
        f"`board {arguments[0]}` {why}; check that the `board` first on PATH is this Board",
        {"code": code if isinstance(code, str) else None, "stderr": ran.stderr.decode(errors="replace")[-4096:]},
    )


def _refused(arguments: tuple[str, ...], code: str, message: str, hint: object) -> PluginError:
    then = f"; {hint}" if isinstance(hint, str) and hint else ""
    return PluginError(
        "controls::board::refused",
        f"`board {arguments[0]}` refused with {code}: {message}{then}",
        {"code": code},
    )


def _document(root: str, *arguments: str) -> dict[str, Any]:
    ran = _run(root, "--json", *arguments)
    try:
        parsed: object = json.loads(ran.stdout)
    except ValueError:
        raise _failed(arguments, ran, "printed no JSON document") from None
    if not isinstance(parsed, dict):
        raise _failed(arguments, ran, "printed no JSON document")
    document = cast("dict[str, Any]", parsed)
    if ran.returncode != 0:
        code = document.get("code")
        if document.get("$type") == "board/error/v1" and isinstance(code, str):
            raise _refused(arguments, code, str(document.get("message", "")), document.get("hint"))
        raise _failed(arguments, ran, "failed without an error document")
    return document


def query(root: str, sql: str) -> list[list[Any]]:
    """The rows of one `board query`, all of them."""
    report = _document(root, "query", "--limit", str(LIMIT), sql)
    if report.get("truncated") is not False or not isinstance(report.get("rows"), list):
        raise PluginError("controls::board::truncated", f"a query returned more than {LIMIT} rows")
    rows: list[list[Any]] = report["rows"]
    return rows


def literal(text: str) -> str:
    """`text` as an SQL string literal. `board query` takes no parameters."""
    return "'" + text.replace("'", "''") + "'"


def controls(root: str) -> dict[str, ControlRecord]:
    """Every control in the board's own tree this plugin can read, by its logical path.

    That includes a control Board marks degraded only because of the tree,
    such as one with a dangling reference; an invalid control is left out.
    """
    rows = query(
        root,
        "SELECT logical, body, status FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND source = 'worktree'",
    )
    found: dict[str, ControlRecord] = {}
    for logical, body, status in rows:
        try:
            found[logical] = ControlRecord.model_validate_json(body)
        except ValidationError as error:
            if status != "ok":
                continue
            raise PluginError(
                "controls::control::unreadable",
                f"{logical} is valid in the board but not a control this plugin reads: {error}",
                {"control": logical},
            ) from None
    return found


def degraded(root: str) -> list[list[str]]:
    """Each control in the board's own tree that Board marks degraded, with its code, by path."""
    return query(
        root,
        "SELECT logical, code FROM records"  # noqa: S608 - the one value is a constant
        f" WHERE type = {literal(CONTROL_TYPE)} AND status != 'ok' AND source = 'worktree' ORDER BY logical",
    )


def refs(root: str, path: str) -> dict[str, Any] | None:
    """`board refs` for `path`: where it resolves and what reaches it, or `None` when nothing is there."""
    try:
        return _document(root, "refs", "--all", path)
    except PluginError as error:
        if error.code == "controls::board::refused" and error.fields.get("code") == "board::path::not_found":
            return None
        raise


def display(root: str, path: str) -> str:
    """The display path of the entry at `path`, with every link replaced by its target."""
    report = refs(root, path)
    if report is None:
        raise PluginError("controls::board::failed", f"nothing is at {path}")
    shown: str = report["location"]["display"]
    return shown


def referencing(report: dict[str, Any]) -> set[str]:
    """The records in a `board refs` report whose `applies_to` reaches its path."""
    return {
        incoming["logical"]
        for incoming in report["to"]
        if incoming["logical"] and any(pointer.startswith("/applies_to/") for pointer in incoming["pointers"])
    }


def read(root: str, path: str) -> bytes:
    """The stored bytes of the file at `path`."""
    ran = _run(root, "get", path)
    if ran.returncode != 0:
        # Without `--json`, Board prints its error as `error: <code>: <message>` and `hint: <hint>` lines.
        lines = dict(line.split(": ", 1) for line in ran.stderr.decode(errors="replace").splitlines() if ": " in line)
        code, _, message = lines.get("error", "").partition(": ")
        if code.startswith("board::"):
            raise _refused(("get",), code, message, lines.get("hint"))
        raise _failed(("get",), ran, f"cannot read {path}")
    return ran.stdout
