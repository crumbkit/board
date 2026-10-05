"""Running the real `board` as a child in the board's root, as every plugin must.

A plugin reads Board's `entries` only through `board query`, and calls other
plugins only through `board call`, so the same checks, limits and logs apply.
"""

import json
import subprocess
from dataclasses import dataclass
from typing import Any, cast

from board_plugin.protocol import PluginError

# The most rows one query returns.
LIMIT = 10_000


def literal(text: str) -> str:
    """`text` as an SQL string literal. `board query` takes no parameters."""
    return "'" + text.replace("'", "''") + "'"


@dataclass(frozen=True)
class Board:
    """The board at `root`, read on behalf of `plugin`, whose name starts each error code."""

    root: str
    plugin: str

    def document(self, *arguments: str) -> dict[str, Any]:
        """The one JSON document `board --json <arguments>` prints.

        Board's own error is `<plugin>::board::refused`, with Board's code and
        hint. Anything else that is not a document is `<plugin>::board::failed`.
        """
        ran = self._run("--json", *arguments)
        try:
            parsed: object = json.loads(ran.stdout)
        except ValueError:
            raise self._failed(arguments, ran, "printed no JSON document") from None
        if not isinstance(parsed, dict):
            raise self._failed(arguments, ran, "printed no JSON document")
        document = cast("dict[str, Any]", parsed)
        if ran.returncode != 0:
            code = document.get("code")
            if document.get("$type") == "board/error/v1" and isinstance(code, str):
                raise self._refused(arguments, code, str(document.get("message", "")), document.get("hint"))
            raise self._failed(arguments, ran, "failed without an error document")
        return document

    def query(self, sql: str, limit: int = LIMIT) -> tuple[list[list[Any]], bool]:
        """The rows of one `board query`, at most `limit`, and whether more were left out."""
        report = self.document("query", "--limit", str(limit), sql)
        rows, truncated = report.get("rows"), report.get("truncated")
        if not isinstance(rows, list) or not isinstance(truncated, bool):
            raise self._failed(("query",), None, "printed a report without rows")
        return cast("list[list[Any]]", rows), truncated

    def rows(self, sql: str) -> list[list[Any]]:
        """Every row of one `board query`. More than `LIMIT` is `<plugin>::board::truncated`."""
        rows, truncated = self.query(sql)
        if truncated:
            raise PluginError(
                f"{self.plugin}::board::truncated",
                f"a query returned more than {LIMIT} rows; narrow what the board holds or what the query asks",
            )
        return rows

    def refs(self, path: str) -> dict[str, Any] | None:
        """`board refs --all` for `path`: where it resolves and what reaches it, or `None` when nothing is there."""
        try:
            return self.document("refs", "--all", path)
        except PluginError as error:
            if error.code == f"{self.plugin}::board::refused" and error.fields.get("code") == "board::path::not_found":
                return None
            raise

    def get(self, path: str) -> bytes:
        """The stored bytes of the file at `path`."""
        ran = self._run("get", path)
        if ran.returncode != 0:
            # Without `--json`, Board prints its error as `error: <code>: <message>` and `hint: <hint>` lines.
            lines = dict(
                line.split(": ", 1) for line in ran.stderr.decode(errors="replace").splitlines() if ": " in line
            )
            code, _, message = lines.get("error", "").partition(": ")
            if code.startswith("board::"):
                raise self._refused(("get",), code, message, lines.get("hint"))
            raise self._failed(("get",), ran, f"cannot read {path}")
        return ran.stdout

    def _run(self, *arguments: str) -> subprocess.CompletedProcess[bytes]:
        try:
            return subprocess.run(  # noqa: S603 - the program is Board itself, found on PATH
                ["board", *arguments],  # noqa: S607
                cwd=self.root,
                capture_output=True,
                check=False,
            )
        except OSError as error:
            raise PluginError(f"{self.plugin}::board::failed", f"board cannot be run: {error}") from error

    def _failed(
        self, arguments: tuple[str, ...], ran: subprocess.CompletedProcess[bytes] | None, why: str
    ) -> PluginError:
        stderr = ran.stderr.decode(errors="replace")[-4096:] if ran is not None else ""
        return PluginError(
            f"{self.plugin}::board::failed",
            f"`board {arguments[0]}` {why}; check that the `board` first on PATH is this Board",
            {"stderr": stderr},
        )

    def _refused(self, arguments: tuple[str, ...], code: str, message: str, hint: object) -> PluginError:
        then = f"; {hint}" if isinstance(hint, str) and hint else ""
        return PluginError(
            f"{self.plugin}::board::refused",
            f"`board {arguments[0]}` refused with {code}: {message}{then}",
            {"code": code},
        )
