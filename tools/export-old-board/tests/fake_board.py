#!/usr/bin/env python3
"""A stand-in for the old `board` that prints `objects list` and `objects show` JSON.

FAKE_BOARD_FIXTURE names a JSON file:
  {"paging": "after" | "cursor", "fail": [subject...], "objects": [ObjectState...]}
Shapes follow crumbkit/board crates/board-cli/src/boardcli/handlers.rs
(list_value, indexed_list_value, show_value, Page, ListItem, ReadSource).
"""

import json
import os
import sys
from pathlib import Path

RECORD_SET = "a" * 64


def fail(message: str) -> None:
    print(f"board: {message}", file=sys.stderr)
    sys.exit(1)


def emit(value: object) -> None:
    # The real printer writes compact ASCII JSON with escapes (boardcli/output.rs).
    print(json.dumps(value, ensure_ascii=True, separators=(",", ":"), sort_keys=True))


def option(args: list[str], name: str) -> str | None:
    if name in args:
        return args[args.index(name) + 1]
    return None


def source() -> dict:
    return {"corpus_root": "/fake", "record_set_sha256": RECORD_SET, "record_count": 1}


def list_page(fixture: dict, args: list[str]) -> None:
    limit = int(option(args, "--limit") or 100)
    after = option(args, "--after")
    cursor = option(args, "--cursor")
    if limit > 256 or (after is None) != (option(args, "--expect-record-set") is None):
        fail("invalid arguments")
    wanted = option(args, "--type")
    # handlers.rs:4877: drops only when asked for by type.
    states = [
        state
        for state in fixture["objects"]
        if (wanted is None and not state["subject"].startswith("drop:"))
        or (wanted is not None and state["subject"].partition(":")[0] == wanted)
    ]
    start = 0
    position = after if fixture["paging"] == "after" else cursor
    if position is not None:
        start = next(index for index, state in enumerate(states) if state["subject"] == position) + 1
    page = states[start : start + limit]
    truncated = start + limit < len(states)
    last = page[-1]["subject"] if truncated else None
    by_after = fixture["paging"] == "after"
    info = {
        "limit": limit,
        "returned": len(page),
        "truncated": truncated,
        "next_after": last if by_after else None,
        "expect_record_set": RECORD_SET if truncated and by_after else None,
    }
    if truncated and not by_after:
        info["next_cursor"] = last
    items = [
        {
            "subject": state["subject"],
            "object_type": state["subject"].partition(":")[0],
            "name": state["subject"].partition(":")[2],
            "standing": state["standing"],
            "authority_limitation": None,
            "head_ids": state["head_ids"],
            "state_record_count": 1,
            "observation_count": 0,
            "outcomes": {"used": 0, "stale": 0, "insufficient": 0},
            "summary": state["summary"],
        }
        for state in page
    ]
    emit(
        {
            "schema_version": 1,
            "operation": "objects.list",
            "source": source(),
            "filter": {"object_type": wanted} if wanted else {},
            "page": info,
            "objects": items,
        }
    )


def show(fixture: dict, subject: str) -> None:
    if subject in fixture.get("fail", []):
        fail("object show failed")
    state = next((state for state in fixture["objects"] if state["subject"] == subject), None)
    if state is None:
        fail("object not found")
    emit(
        {
            "schema_version": 1,
            "operation": "objects.show",
            "source": source(),
            "object": state,
            "refs": {},
            "reviews": None,
            "issues": [],
            "unresolved": [],
        }
    )


def main() -> None:
    fixture = json.loads(Path(os.environ["FAKE_BOARD_FIXTURE"]).read_text(encoding="utf-8"))
    args = sys.argv[1:]
    if option(args, "--format") != "json":
        fail("fake board only prints json")
    if args[:2] == ["objects", "list"]:
        list_page(fixture, args)
    elif args[:2] == ["objects", "show"]:
        show(fixture, args[2])
    else:
        fail("unsupported command")


if __name__ == "__main__":
    main()
