"""Running the real `board` as a child in the board's root, as every plugin must.

A plugin reads Board's `entries` only through `board query`, and calls other
plugins only through `board call`, so the same checks, limits and logs apply.
"""

import base64
import binascii
import json
import subprocess
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any, TypeVar, cast

from board_plugin.protocol import PluginError

T = TypeVar("T")

# The most rows one query returns.
LIMIT = 10_000
# The most paths one `board refs` or `board get` answers: Board's MAX_LIMIT.
PATHS_PER_RUN = 10_000
# What a query report lists for a plugin whose database it could not attach.
NO_DATABASE = "board::plugin::no_database"
# What `board get` answers for a path it left unread once its run printed its byte budget.
OVER_BUDGET = "board::get::over_budget"
# What `board refs` answers for a path it left unanswered once its run printed its byte budget.
REFS_OVER_BUDGET = "board::refs::over_budget"


def literal(text: str) -> str:
    """`text` as an SQL string literal. `board query` takes no parameters."""
    return "'" + text.replace("'", "''") + "'"


@dataclass(frozen=True)
class Board:
    """The board at `root`, read on behalf of `plugin`, whose name starts each error code."""

    root: str
    plugin: str
    # The most paths one run is asked; more are split over several runs.
    paths_per_run: int = PATHS_PER_RUN

    def document(self, *arguments: str, partial: str | None = None) -> dict[str, Any]:
        """The one JSON document `board --json <arguments>` prints.

        Board's own error is `<plugin>::board::refused`, with Board's code and
        hint. Anything else that is not a document is `<plugin>::board::failed`.
        A document whose `$type` is `partial` is the answer even when Board
        exits 1, as `refs` does when a path it was asked for is refused.
        """
        return self._read(*arguments, partial=partial)[0]

    def _read(
        self, *arguments: str, partial: str | None = None
    ) -> tuple[dict[str, Any], subprocess.CompletedProcess[bytes]]:
        """The document `document` reads, and the run that printed it."""
        ran = self._run("--json", *arguments)
        try:
            parsed: object = json.loads(ran.stdout)
        except ValueError:
            raise self._failed(arguments, ran, "printed no JSON document") from None
        if not isinstance(parsed, dict):
            raise self._failed(arguments, ran, "printed no JSON document")
        document = cast("dict[str, Any]", parsed)
        if ran.returncode != 0 and (partial is None or document.get("$type") != partial):
            code = document.get("code")
            if document.get("$type") == "board/error/v1" and isinstance(code, str):
                raise self._refused(arguments, code, str(document.get("message", "")), document.get("hint"))
            raise self._failed(arguments, ran, "failed without an error document")
        return document, ran

    def query(self, sql: str, limit: int = LIMIT) -> tuple[list[list[Any]], bool]:
        """The rows of one `board query`, at most `limit`, and whether more were left out.

        When the report lists this plugin's own extraction as failed, its rows
        may be stale, so that is `<plugin>::board::refused` with the failure's
        code. `board::plugin::no_database` is not an extraction failure: the
        database was not attached, and a query that reads it fails on its own.
        """
        report = self.document("query", "--limit", str(limit), sql)
        rows, truncated, plugins = report.get("rows"), report.get("truncated"), report.get("plugins")
        if not isinstance(rows, list) or not isinstance(truncated, bool) or not isinstance(plugins, list):
            raise self._failed(("query",), None, "printed a report without rows")
        for failed in cast("list[dict[str, Any]]", plugins):
            code = failed.get("code")
            if failed.get("plugin") == self.plugin and isinstance(code, str) and code != NO_DATABASE:
                why = f"{self.plugin} could not extract, so its rows may be stale: {failed.get('message')}"
                raise self._refused(("query",), code, why, None)
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

    def refs(
        self, paths: Sequence[str], *, missing: bool = False, fields: Sequence[str] = ()
    ) -> list[dict[str, Any] | None]:
        """`board refs --all` for `paths`, from as few runs as Board allows.

        Each answer, in order, is where the path resolves and what reaches it,
        or `None` when nothing is there. With `missing`, a path with nothing
        there yet reports where a file there would be. Each of `fields`, a
        JSON pointer, adds to each reference the path makes its target's value
        there. Any other refusal of a path raises its code. A run takes at
        most `paths_per_run` paths and leaves the paths after its byte budget
        unanswered, so those are asked again in the next run. No paths start
        no run.
        """
        flags = ["--missing"] if missing else []
        for field in fields:
            flags += ["--field", field]
        return self._each(paths, lambda left: self._refs_run(left, flags))

    def _refs_run(self, paths: list[str], flags: list[str]) -> list[dict[str, Any] | None]:
        """The answers of one `board refs` run, up to the first path it left over its budget."""
        report, ran = self._read("refs", "--all", *flags, "--", *paths, partial="board/refs-report/v1")
        answers = report.get("answers")
        if (
            not isinstance(answers, list)
            or len(cast("list[Any]", answers)) != len(paths)
            or not all(isinstance(answer, dict) for answer in cast("list[Any]", answers))
        ):
            raise self._failed(("refs",), ran, "printed a report without one answer per path")
        found: list[dict[str, Any] | None] = []
        for answer in cast("list[dict[str, Any]]", answers):
            code = answer.get("code")
            if answer.get("$type") != "board/error/v1":
                found.append(answer)
            elif code == REFS_OVER_BUDGET:
                break
            elif code == "board::path::not_found":
                found.append(None)
            else:
                raise self._refused(("refs",), str(code), str(answer.get("message", "")), answer.get("hint"))
        if not found:
            raise self._failed(("refs",), ran, f"left every path unanswered as {REFS_OVER_BUDGET}")
        return found

    def _each(self, paths: Sequence[str], ask: Callable[[list[str]], list[T]]) -> list[T]:
        """What `ask` answers for each of `paths`, in order, asking again for the paths a run left over its budget."""
        answered: list[T] = []
        for run in self._runs(paths):
            left = list(run)
            while left:
                some = ask(left)
                answered.extend(some)
                left = left[len(some) :]
        return answered

    def _runs(self, paths: Sequence[str]) -> list[Sequence[str]]:
        """`paths` split into runs of at most `paths_per_run`. One str is a path, not a sequence, so it is refused."""
        if isinstance(paths, str):
            message = f"paths must be a sequence of paths, not the one str {paths!r}; pass [{paths!r}]"
            raise TypeError(message)
        step = self.paths_per_run
        return [paths[start : start + step] for start in range(0, len(paths), step)]

    def get(self, paths: Sequence[str]) -> list[bytes | PluginError]:
        """The stored bytes of the file at each of `paths`, from as few `board get` runs as Board allows.

        Each answer, in order, is the file's bytes, or the refusal of that path
        as a `<plugin>::board::refused` error, returned rather than raised so
        the caller decides what one refused path means. A run takes at most
        `paths_per_run` paths and leaves the paths after its byte budget
        unread, so those are asked again in the next run. No paths start no run.
        """
        return self._each(paths, self._get_run)

    def _get_run(self, paths: list[str]) -> list[bytes | PluginError]:
        """The answers of one `board get` run, up to the first path it left over its budget.

        A run that answers none of its paths would be asked again forever, so
        it is `<plugin>::board::failed`.
        """
        report, ran = self._read("get", "--", *paths, partial="board/get-report/v1")
        answers = report.get("answers")
        found = [_file(answer) for answer in cast("list[Any]", answers)] if isinstance(answers, list) else []
        if len(found) != len(paths) or any(answer is None for answer in found):
            raise self._failed(("get",), ran, "printed a report without one file or refusal per path")
        read: list[bytes | PluginError] = []
        for answer in found:
            if isinstance(answer, dict) and answer.get("code") == OVER_BUDGET:
                break
            if isinstance(answer, dict):
                why, hint = str(answer.get("message", "")), answer.get("hint")
                read.append(self._refused(("get",), str(answer.get("code")), why, hint))
            elif answer is not None:
                read.append(answer)
        if not read:
            raise self._failed(("get",), ran, f"left every path unread as {OVER_BUDGET}")
        return read

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


def _file(answer: object) -> bytes | dict[str, Any] | None:
    """A `board get` answer's bytes, the refusal it is, or `None` when it is neither."""
    if not isinstance(answer, dict):
        return None
    answer = cast("dict[str, Any]", answer)
    text, encoded = answer.get("text"), answer.get("base64")
    if answer.get("$type") == "board/error/v1":
        return answer
    if isinstance(text, str):
        return text.encode()
    try:
        return base64.b64decode(encoded, validate=True) if isinstance(encoded, str) else None
    except binascii.Error:
        return None
