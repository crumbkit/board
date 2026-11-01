"""Rewrite a board's `plans/decision/v1` records as `decisions/decision/v1`, and its evals without a stored status.

Run it from a Board checkout, with the `board` that has the decisions plugin built in on PATH, after
`board plugin enable` has installed the new schemas of decisions, evals and plans in the board:

    uv run --project plugins/decisions python plugins/decisions/migrate.py --session local_<id> --dry-run <board>

Each decision keeps its path. `refs` becomes `basis`, `review` and `written_by` are dropped, and the other fields
are copied. A stub, which a hook wrote with `written_by`, that still has an empty `why` gets one that says no reason
was recorded. Every decision whose `review` was `wanted`, and every stub, waits on the owner: the script writes one
open `plans/question/v1` per project, beside the project's `decisions/` directory, whose `for` lists them.

Each eval drops `status`, takes `abandoned` from an `abandoned` status (its reason is the `conclusion`), and renames
`decision` to `informs` and `mounts` to `repos`. The script prints each eval whose derived status differs from the
status it stored.

With `--dry-run` it prints what it would write and writes nothing. Without it, one `board put --batch` writes every
record, in one commit when the board commits its puts. The script stays in the repository until the runs mod stops
writing `plans/decision/v1` stubs; the change that stops them runs it once more for the stubs written meanwhile.
"""

import argparse
import re
import secrets
import string
import sys
from collections.abc import Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from board_plugin.board import Board
from board_plugin.migrate import Line, Old, migrate, olds, put
from board_plugin.protocol import PluginError

OLD_DECISION = "plans/decision/v1"
DECISION = "decisions/decision/v1"
QUESTION = "plans/question/v1"
EVAL = "evals/eval/v1"
SESSION = re.compile(r"^local_[0-9a-f-]+$")
NO_REASON = "no reason was recorded"


def decision(old: dict[str, Any]) -> dict[str, Any]:
    """The `decisions/decision/v1` an old decision becomes."""
    new: dict[str, Any] = {"$type": DECISION}
    for name, value in old.items():
        if name in ("$type", "review", "written_by"):
            continue
        new["basis" if name == "refs" else name] = value
    if not new.get("why"):
        urls: list[str] = old.get("urls") or []
        opened = f"the pull request {urls[0]}" if urls else "its pull request"
        new["why"] = f"This stub was written when {opened} was opened; {NO_REASON}."
    return new


def waits(old: dict[str, Any]) -> bool:
    """Whether an old decision waits on the owner: its review was wanted, or a hook wrote it as a stub."""
    return old.get("review") == "wanted" or "written_by" in old


def status(record: dict[str, Any]) -> str:
    """The status an eval's fields derive."""
    if "abandoned" in record:
        return "abandoned"
    if "verdict" in record:
        return "concluded"
    return "running" if record.get("runs") else "planned"


def evaluation(old: dict[str, Any]) -> dict[str, Any]:
    """The eval an old eval becomes."""
    renamed = {"decision": "informs", "mounts": "repos"}
    new = {renamed.get(name, name): value for name, value in old.items() if name != "status"}
    if old.get("status") == "abandoned":
        new["abandoned"] = old.get("conclusion") or f"It was abandoned before evals derived their status; {NO_REASON}."
    return new


def question_id(now: datetime) -> str:
    """A sortable UTC time and a short random suffix, such as `20261024T1830Z-k3`."""
    suffix = "".join(secrets.choice(string.ascii_lowercase + string.digits) for _ in range(2))
    return f"{now.strftime('%Y%m%dT%H%MZ')}-{suffix}"


def plan(board: Board, session: str, now: datetime) -> tuple[list[Line], list[str]]:
    """The batch lines the migration writes, and one line of text for each thing it does."""
    lines: list[Line] = []
    said: list[str] = []
    waiting: dict[str, list[str]] = {}
    for old in olds(board, [OLD_DECISION]):
        lines.append(put(old, decision(old.record)))
        if waits(old.record):
            # /projects/<project>/decisions/<id> waits under /projects/<project>.
            waiting.setdefault(old.path.rsplit("/", 2)[0], []).append(old.path)
            said.append(f"rewrite {old.path} as {DECISION}; it waits on the owner")
        else:
            said.append(f"rewrite {old.path} as {DECISION}")
    for project, paths in sorted(waiting.items()):
        path = f"{project}/questions/{question_id(now)}"
        record = {
            "$type": QUESTION,
            "question": f"Do you accept the {len(paths)} decisions listed in `for`? Each was marked for your review, "
            "or a hook wrote it as a stub when a pull request was opened.",
            "session": session,
            "asked": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "status": "open",
            "for": paths,
        }
        lines.append({"path": path, "expect": "none", "record": record})
        said.append(f"ask {path} to review {len(paths)} decision{'' if len(paths) == 1 else 's'}")
    for old in olds(board, [EVAL]):
        lines.extend(evals(old, said))
    return lines, said


def evals(old: Old, said: list[str]) -> list[Line]:
    """The line that rewrites one eval, if it changes, and what to say about it."""
    new = evaluation(old.record)
    if new == old.record:
        return []
    said.append(f"rewrite eval {old.path}")
    stored, derived = old.record.get("status"), status(new)
    if stored != derived:
        said.append(f"eval {old.path} stored status {stored} and now derives {derived}")
    return [put(old, new)]


def main(arguments: Sequence[str] | None = None) -> int:
    """Run the migration, and return the exit status."""
    parser = argparse.ArgumentParser(
        description="Rewrite plans/decision/v1 records and evals for the decisions plugin."
    )
    parser.add_argument("board", type=Path, help="the board's directory")
    parser.add_argument("--session", required=True, help="the running session's local_ id, for the questions")
    parser.add_argument("--dry-run", action="store_true", help="print what would be written and write nothing")
    given = parser.parse_args(arguments)
    if not SESSION.match(given.session):
        print(f"error: --session {given.session!r} is not a local_ session id, such as local_0a1b", file=sys.stderr)
        return 2
    board = Board(str(given.board.expanduser().resolve()), "decisions")
    try:
        lines, said = plan(board, given.session, datetime.now(UTC))
        result = migrate(board, lines, apply=not given.dry_run)
    except PluginError as error:
        print(f"error: {error.code}: {error}", file=sys.stderr)
        return 1
    for line in said:
        print(line)
    if given.dry_run:
        print(f"dry run: {result['total']} records would be written; nothing was written")
    else:
        print(f"wrote {result['total']} records; commit {result['commit']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
