# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Export every current object head of the old `board` into a new board.

Throwaway: delete this directory once the export repository exists (PLAN.md M1).
"""

import argparse
import json
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

# The old CLI refuses a larger page (board-cli/src/cli.rs MAX_PAGE_LIMIT).
PAGE_LIMIT = 256
MAX_SEGMENT_BYTES = 255
MAX_PATH_BYTES = 4096
TYPE_CHARS = frozenset("abcdefghijklmnopqrstuvwxyz0123456789-")


class OldBoardError(Exception):
    pass


def canonical(value: object) -> bytes:
    """docs/architecture.md, Canonical form."""
    text = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)
    return (text + "\n").encode()


def run_board(board: str, store: str | None, args: list[str]) -> dict:
    command = [board, *args, "--format", "json"]
    if store is not None:
        command += ["--store", store]
    done = subprocess.run(command, capture_output=True, check=False)
    if done.returncode != 0:
        detail = done.stderr.decode(errors="replace").strip() or done.stdout.decode(errors="replace").strip()
        raise OldBoardError(f"`{' '.join(args)}` exited {done.returncode}: {detail[:500]}")
    return json.loads(done.stdout)


def list_subjects(board: str, store: str | None, filters: list[str]) -> list[str]:
    """Every subject `board objects list` returns, following both of its pagination forms."""
    subjects: list[str] = []
    paging: list[str] = []
    while True:
        page = run_board(board, store, ["objects", "list", *filters, "--limit", str(PAGE_LIMIT), *paging])
        subjects += [item["subject"] for item in page["objects"]]
        info = page["page"]
        if not info["truncated"]:
            return subjects
        if info.get("next_cursor"):
            paging = ["--cursor", info["next_cursor"]]
        else:
            paging = ["--after", info["next_after"], "--expect-record-set", info["expect_record_set"]]


def path_problem(object_type: str, name: str) -> str | None:
    """Why `/<type>/<name>` is not a logical path under docs/architecture.md, or None."""
    if not object_type or not set(object_type) <= TYPE_CHARS:
        return "type_invalid"
    for segment in [object_type, *name.split("/")]:
        if segment in ("", ".", ".."):
            return "path_malformed"
        if len(segment.encode()) > MAX_SEGMENT_BYTES:
            return "path_malformed"
        if any(char == "/" or ord(char) < 0x20 or 0x7F <= ord(char) < 0xA0 for char in segment):
            return "path_malformed"
    if len(f"/{object_type}/{name}".encode()) > MAX_PATH_BYTES:
        return "path_malformed"
    return None


def case_collisions(paths: dict[str, tuple[str, ...]]) -> set[str]:
    """Subjects whose file or a directory on its path differs from another name only by ASCII case.

    Distinct subjects map to distinct files, and a record `x.json` may sit beside a directory `x/`.
    """
    names: set[tuple[str, ...]] = set()
    for segments in paths.values():
        names.update(segments[:depth] for depth in range(1, len(segments)))
        names.add((*segments[:-1], segments[-1] + ".json"))
    by_folded: dict[tuple[str, ...], set[tuple[str, ...]]] = defaultdict(set)
    for name in names:
        by_folded[(*name[:-1], name[-1].lower())].add(name)
    bad = {name for group in by_folded.values() if len(group) > 1 for name in group}
    found: set[str] = set()
    for subject, segments in paths.items():
        on_disk = (*segments[:-1], segments[-1] + ".json")
        if any(on_disk[:depth] in bad for depth in range(1, len(on_disk) + 1)):
            found.add(subject)
    return found


def plan(subjects: list[str]) -> tuple[dict[str, tuple[str, ...]], list[dict[str, str]]]:
    """Split distinct subjects into mappable paths and unmappable entries."""
    paths: dict[str, tuple[str, ...]] = {}
    refused: list[dict[str, str]] = []
    for subject in subjects:
        object_type, colon, name = subject.partition(":")
        problem = "subject_malformed" if not colon else path_problem(object_type, name)
        if problem:
            refused.append({"subject": subject, "reason": problem})
        else:
            paths[subject] = (object_type, *name.split("/"))
    for subject in case_collisions(paths):
        del paths[subject]
        refused.append({"subject": subject, "reason": "case_collision"})
    return paths, refused


def show(board: str, store: str | None, subject: str) -> dict:
    return run_board(board, store, ["objects", "show", subject])


def export(board: str, store: str | None, out: Path, jobs: int) -> dict:
    # The default list hides drops (old board-cli/src/boardcli/handlers.rs:4877).
    listed = list_subjects(board, store, [])
    drops = list_subjects(board, store, ["--type", "drop"])
    subjects = list(dict.fromkeys(listed + drops))
    paths, refused = plan(subjects)
    shown: dict[str, dict] = {}
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {subject: pool.submit(show, board, store, subject) for subject in paths}
        for subject, future in futures.items():
            try:
                shown[subject] = future.result()
            except (OldBoardError, json.JSONDecodeError) as error:
                refused.append({"subject": subject, "reason": "show_failed", "detail": str(error)})
    per_type: Counter[str] = Counter()
    record_sets: set[str] = set()
    for subject, value in shown.items():
        segments = paths[subject]
        if "$type" in value["object"]:
            refused.append({"subject": subject, "reason": "type_key_present"})
            continue
        record = {**value["object"], "$type": f"legacy/{segments[0]}/v1"}
        target = out.joinpath(*segments[:-1], segments[-1] + ".json")
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(canonical(record))
        except OSError as error:
            # A logical segment may be 255 bytes, but its record file adds ".json".
            refused.append({"subject": subject, "reason": "write_failed", "detail": str(error)})
            continue
        per_type[segments[0]] += 1
        record_sets.add(value["source"]["record_set_sha256"])
    for object_type in per_type:
        schema = out / ".board" / "types" / "legacy" / object_type / "v1.schema.json"
        schema.parent.mkdir(parents=True, exist_ok=True)
        schema.write_bytes(canonical({"type": "object"}))
    (out / ".board" / "board.json").parent.mkdir(parents=True, exist_ok=True)
    config = {"$type": "board/config/v1", "commit_on_put": True, "plugins": []}
    (out / ".board" / "board.json").write_bytes(canonical(config))
    record_count = sum(per_type.values())
    report = {
        "listed_count": len(listed),
        "drop_listed_count": len(drops),
        "subject_count": len(subjects),
        "record_count": record_count,
        "per_type": dict(per_type),
        "per_reason": dict(Counter(entry["reason"] for entry in refused)),
        "unmappable": sorted(refused, key=lambda entry: (entry["subject"], entry["reason"])),
        "show_record_sets": sorted(record_sets),
    }
    if record_count != len(subjects) - len(refused):
        raise OldBoardError(
            f"count invariant broken: {record_count} records, {len(subjects)} subjects, {len(refused)} unmappable"
        )
    (out / "export-report.json").write_bytes(canonical(report))
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out", type=Path, help="a fresh git repository: empty apart from .git")
    parser.add_argument("--board", help="the old board executable; default: `board` on PATH")
    parser.add_argument("--store", help="passed through to the old board as --store")
    parser.add_argument("--jobs", type=int, default=8, help="concurrent `objects show` calls")
    args = parser.parse_args()
    board = args.board or shutil.which("board")
    if board is None:
        parser.error("no `board` on PATH; pass --board")
    if not args.out.is_dir() or any(entry.name != ".git" for entry in args.out.iterdir()):
        parser.error(f"{args.out} must be an existing directory holding nothing but .git")
    try:
        report = export(board, args.store, args.out, args.jobs)
    except OldBoardError as error:
        print(f"export failed: {error}", file=sys.stderr)
        return 1
    summary = {key: value for key, value in report.items() if key != "unmappable"}
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
