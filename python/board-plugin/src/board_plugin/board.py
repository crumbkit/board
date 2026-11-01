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
# What `board get` and `board refs` answer for the paths they left out once a run printed its
# byte budget, and what a query report carries when it left rows out that would pass it.
OVER_BUDGET = "board::output::over_budget"
# The schema of the document Board prints when a command fails.
FAILURE = "board/failure/v1"
# The codes Board gives a valid record whose reference dangles or reaches the wrong type, as
# board_plugin::shape::TREE_CODES lists them; a query admits a record with one of them as valid.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")


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
    # Board's own flags that start every run, such as `--personal` to run as the personal board.
    flags: tuple[str, ...] = ()

    def document(
        self, *arguments: str, partial: str | None = None, stdin: bytes = b"", timeout: float | None = None
    ) -> dict[str, Any]:
        """The one JSON document `board --json <arguments>` prints, given `stdin`, as a put reads its input.

        Board's own error is `<plugin>::board::refused`, with Board's code and
        hint. Anything else that is not a document is `<plugin>::board::failed`.
        A document whose `schema` is `partial` is the answer even when Board
        exits 1, as `refs` does when a path it was asked for is refused.
        A run still going after `timeout` seconds is left to finish on its
        own, and is `<plugin>::board::timed_out`.
        """
        return self._read(*arguments, partial=partial, stdin=stdin, timeout=timeout)[0]

    def _read(
        self, *arguments: str, partial: str | None = None, stdin: bytes = b"", timeout: float | None = None
    ) -> tuple[dict[str, Any], subprocess.CompletedProcess[bytes]]:
        """The document `document` reads, and the run that printed it."""
        ran = self._run("--json", *arguments, stdin=stdin, timeout=timeout)
        try:
            parsed: object = json.loads(ran.stdout)
        except ValueError:
            raise self._failed(arguments, ran, "printed no JSON document") from None
        if not isinstance(parsed, dict):
            raise self._failed(arguments, ran, "printed no JSON document")
        document = cast("dict[str, Any]", parsed)
        if ran.returncode != 0 and (partial is None or document.get("schema") != partial):
            code = document.get("code")
            if document.get("schema") == FAILURE and isinstance(code, str):
                raise self._refused(arguments, code, str(document.get("message", "")), _hint(document), document)
            raise self._failed(arguments, ran, "failed without an error document")
        return document, ran

    def query(self, sql: str, limit: int = LIMIT) -> tuple[list[list[Any]], bool]:
        """The rows of one `board query`, at most `limit`, and whether more were left out.

        Board leaves rows out past `limit` or at its byte budget, and either
        is `True` here, so the caller can ask for the rest by key.
        """
        return self._query(sql, "--limit", str(limit))

    def _query(self, sql: str, *page: str) -> tuple[list[list[Any]], bool]:
        """`query`'s answer, with `page` the flags that say how many rows to return."""
        report = self.document("query", *page, sql)
        rows, truncated = report.get("data"), report.get("truncated")
        if not isinstance(rows, list) or not isinstance(truncated, bool):
            raise self._failed(("query",), None, "printed a report without rows")
        return cast("list[list[Any]]", rows), truncated

    def rows(self, sql: str) -> list[list[Any]]:
        """Every row of one `board query --all`.

        With `--all`, Board leaves rows out only at its byte budget, so rows
        left out are `<plugin>::board::over_budget`. A first row or a single
        value past the budget is `<plugin>::board::refused`, with Board's code
        `board::output::over_budget`.
        """
        rows, truncated = self._query(sql, "--all")
        if truncated:
            raise PluginError(
                f"{self.plugin}::board::over_budget",
                f"a query's rows passed the 64 MiB one board run prints ({OVER_BUDGET}); select fewer "
                "or smaller columns, or add ORDER BY and page by key, as in WHERE (a, b) > (last a, last b)",
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
        report, ran = self._read("refs", "--all", *flags, "--", *paths, partial="board/refs-report/v2")
        answers = _answers(report, len(paths))
        if answers is None:
            raise self._failed(("refs",), ran, "printed a report without one answer per path")
        found: list[dict[str, Any] | None] = []
        for refused, answer in answers:
            code = answer.get("code")
            if not refused:
                found.append(answer)
            elif code == "board::path::not_found":
                found.append(None)
            else:
                raise self._refused(("refs",), str(code), str(answer.get("message", "")), _hint(answer), answer)
        if not found:
            raise self._failed(("refs",), ran, f"left every path unanswered as {OVER_BUDGET}")
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
        report, ran = self._read("get", "--", *paths, partial="board/get-report/v2")
        answers = _answers(report, len(paths))
        found = [(refused, answer if refused else _file(answer)) for refused, answer in answers or []]
        if answers is None or any(answer is None for _, answer in found):
            raise self._failed(("get",), ran, "printed a report without one file or refusal per path")
        read: list[bytes | PluginError] = []
        for refused, answer in found:
            if refused and isinstance(answer, dict):
                why, hint = str(answer.get("message", "")), _hint(answer)
                read.append(self._refused(("get",), str(answer.get("code")), why, hint, answer))
            elif isinstance(answer, bytes):
                read.append(answer)
        if not read:
            raise self._failed(("get",), ran, f"left every path unread as {OVER_BUDGET}")
        return read

    def _run(
        self, *arguments: str, stdin: bytes = b"", timeout: float | None = None
    ) -> subprocess.CompletedProcess[bytes]:
        """Run `board <arguments>`, given `stdin`.

        A run still going after `timeout` seconds is left to finish in a session of its own, and is not
        stopped: stopping Board mid-write would leave its lock behind, and every later write would be refused.
        """
        command = ["board", *self.flags, *arguments]
        try:
            child = subprocess.Popen(  # noqa: S603 - the program is Board itself, found on PATH
                command,
                cwd=self.root,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=timeout is not None,
            )
        except OSError as error:
            raise PluginError(f"{self.plugin}::board::failed", f"board cannot be run: {error}") from error
        try:
            stdout, stderr = child.communicate(stdin, timeout=timeout)
        except subprocess.TimeoutExpired:
            why = (
                f"`board {' '.join(arguments)}` ran past {timeout:g} s and was left to finish on its own; "
                "call again, or check its remote"
            )
            raise PluginError(f"{self.plugin}::board::timed_out", why, {"seconds": timeout}) from None
        return subprocess.CompletedProcess(command, child.returncode, stdout, stderr)

    def _named(self, arguments: tuple[str, ...]) -> str:
        """The command a message names: `board`, its flags and the subcommand."""
        return " ".join(("board", *self.flags, arguments[0]))

    def _failed(
        self, arguments: tuple[str, ...], ran: subprocess.CompletedProcess[bytes] | None, why: str
    ) -> PluginError:
        stderr = ran.stderr.decode(errors="replace")[-4096:] if ran is not None else ""
        return PluginError(
            f"{self.plugin}::board::failed",
            f"`{self._named(arguments)}` {why}; check that the `board` first on PATH is this Board",
            {"stderr": stderr},
        )

    def _refused(
        self, arguments: tuple[str, ...], code: str, message: str, hint: object, failure: dict[str, Any] | None = None
    ) -> PluginError:
        """`<plugin>::board::refused`, carrying Board's code and the fields of Board's `failure`, such as `refusals`."""
        then = f"; {hint}" if isinstance(hint, str) and hint else ""
        fields = failure.get("fields") if failure else None
        carried = cast("dict[str, Any]", fields) if isinstance(fields, dict) else {}
        return PluginError(
            f"{self.plugin}::board::refused",
            f"`{self._named(arguments)}` refused with {code}: {message}{then}",
            {**carried, "code": code},
        )


def _hint(failure: dict[str, Any]) -> str | None:
    """What a crumbkit failure says to do next: its hint, or the command it says to run."""
    recovery = failure.get("recovery")
    if not isinstance(recovery, dict):
        return None
    recovery = cast("dict[str, Any]", recovery)
    text, call = recovery.get("text"), recovery.get("call")
    if isinstance(text, str):
        return text
    return f"run `{call}`" if isinstance(call, str) else None


def _answers(report: dict[str, Any], asked: int) -> list[tuple[bool, dict[str, Any]]] | None:
    """Each answer of a batch report, in the order its paths were asked, up to the first left over its budget.

    Board prints the files or references it found in `data` and each refused
    path in `failures` with the `index` of its path. The one failure without
    an index left the paths from `left_out_from` on unanswered. Each answer
    is whether it is a refusal, and the answer. `None` when the report does
    not hold one answer per path.
    """
    data, failures = report.get("data"), report.get("failures")
    if not isinstance(data, list) or not isinstance(failures, list):
        return None
    refused: dict[int, dict[str, Any]] = {}
    answered = asked
    for failure in cast("list[Any]", failures):
        if not isinstance(failure, dict):
            return None
        failure = cast("dict[str, Any]", failure)
        index, fields = failure.get("index"), failure.get("fields")
        left_out_from = cast("dict[str, Any]", fields).get("left_out_from") if isinstance(fields, dict) else None
        if isinstance(index, int):
            refused[index] = failure
        elif failure.get("code") == OVER_BUDGET and isinstance(left_out_from, int):
            answered = left_out_from
        else:
            return None
    data = cast("list[Any]", data)
    found = iter(data)
    if len(data) + len(refused) != answered or any(index >= answered for index in refused):
        return None
    answers: list[tuple[bool, dict[str, Any]]] = []
    for index in range(answered):
        answer = refused.get(index) or next(found)
        if not isinstance(answer, dict):
            return None
        answers.append((index in refused, cast("dict[str, Any]", answer)))
    return answers


def _file(answer: dict[str, Any]) -> bytes | None:
    """A `board get` file's bytes, or `None` when it holds none."""
    text, encoded = answer.get("text"), answer.get("base64")
    if isinstance(text, str):
        return text.encode()
    try:
        return base64.b64decode(encoded, validate=True) if isinstance(encoded, str) else None
    except binascii.Error:
        return None
