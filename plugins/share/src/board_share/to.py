"""The `to` capability: copy a record and everything a reader needs into a mounted board.

The set is the entry at the path, the entries under it, and every entry its
records reference, followed transitively, inside this board's own tree. Only
the references a record's schema declares are followed, including those in a
Markdown file's frontmatter; a Markdown file's links are not. A
reference to a directory reaches the entries under it. A reference that
resolves through a mount or link is not copied, and must resolve to the same
repository in the destination; a link inside the set is refused. The
plugin's own refusals are decided before anything is written, on a dry run
too. Then one `board put --batch`
writes the missing schemas and the set through the mount, which makes one
commit in the mounted board and moves the mount's pin to it.
"""

import json
from typing import Any
from urllib.parse import urlsplit

from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

PLUGIN = "share"
ATTACHMENT = "board/attachment/v1"
# What `board get` answers for a path holding nothing, or only a directory, beside which a record may go.
ABSENT = ("board::path::not_found", "board::record::not_a_record")
# The most paths the output lists (STANDARDS 23); ToOutput's description states it.
LIST_LIMIT = 100

type Line = dict[str, object]


def share_to(request: CallRequest) -> dict[str, object]:
    """Copy `path`'s set into the board mounted at `to`, or list it on a dry run."""
    path, to = str(request.input["path"]), str(request.input["to"])
    here = Board(request.board.root, PLUGIN)
    repository, _ = mount(here, to)
    found, mounted = closure(here, path)
    unresolved(here, to, mounted)
    lines, types = read(here, found, local_attachments=request.input.get("local_attachments") is True)
    held = here.get([to + target for target in lines])
    unchanged = {target for target, now in zip(lines, held, strict=True) if same(lines[target], now)}
    differs = [target for target, now in zip(lines, held, strict=True) if not absent(now) and target not in unchanged]
    schemas = {
        name: parsed(data) for name, data in zip(types, here.get([schema_path(name) for name in types]), strict=True)
    }
    missing: list[str] = []
    for name, now in zip(types, here.get([to + schema_path(name) for name in types]), strict=True):
        if absent(now):
            missing.append(name)
        elif not same({"record": schemas[name]}, now):
            differs.append(schema_path(name))
    refuse("share::target::differs", "the destination already holds something else at", differs)
    copied = [target for target in lines if target not in unchanged]
    commit = None
    if request.input.get("dry_run") is not True and (missing or copied):
        batch = [{"path": to + schema_path(name), "record": schemas[name]} for name in missing]
        batch += [{"path": to + target, "expect": "none", **lines[target]} for target in copied]
        here.document("put", "--batch", stdin="".join(json.dumps(line) + "\n" for line in batch).encode())
        _, commit = mount(here, to)
    return {
        "repository": repository,
        "commit": commit,
        "types": missing,
        "copied": copied[:LIST_LIMIT],
        "total": len(copied),
        "truncated": len(copied) > LIST_LIMIT,
        "unchanged": len(unchanged),
    }


def mount(here: Board, to: str) -> tuple[str, str]:
    """The repository the mount at `to` reaches and the commit its pin names.

    `to` must be the mount record itself: not a path inside a mount, a mount
    inside a mounted board, or a link to a mount, which a batch cannot write
    through.
    """
    [answer] = here.refs([to])
    location: dict[str, Any] = answer["location"] if answer else {}
    pointers: list[dict[str, Any]] = location.get("pointers", [])
    if location.get("path") != "" or len(pointers) != 1 or pointers[0]["repository"] == location["repository"]:
        why = f"{to} is not the mount record of another repository; `board ls {to}` shows what is there"
        raise PluginError("share::mount::not_found", why)
    return str(location["repository"]), str(location["source"])


def closure(here: Board, path: str) -> tuple[list[str], dict[str, str]]:
    """The paths to copy, sorted, and each reference through a mount or link with the repository it reaches.

    Only the references a record's schema declares are followed, and a
    Markdown file's links are not. A copied note's links may dangle in the
    destination, where `board check` reports them.
    """
    found = entries(here, under([path]))
    if not found:
        raise PluginError("share::path::not_found", f"{path} names nothing in this board's own tree")
    mounted: dict[str, str] = {}
    frontier = records(found)
    while frontier:
        reached: set[str] = set()
        for answer in here.refs(frontier):
            references: list[dict[str, Any]] = answer["from"] if answer else []
            for reference in references:
                location = reference.get("location")
                if reference.get("link") or location is None:
                    continue
                if location["pointers"] or location["source"] != "worktree":
                    mounted[str(reference["target"])] = str(location["repository"])
                elif location["display"] not in found:
                    reached.add(str(location["display"]))
        if not reached:
            break
        held = entries(here, f"logical IN ({', '.join(literal(target) for target in reached)})")
        # A reference to a directory has no entry of its own, and reaches the entries under it.
        held |= entries(here, under(sorted(reached - held.keys())))
        new = {target: kind for target, kind in held.items() if target not in found}
        found |= new
        frontier = records(new)
    return sorted(found), mounted


def records(kinds: dict[str, str]) -> list[str]:
    """The paths among `kinds` that hold records, sorted: the entries whose references a share follows."""
    return sorted(target for target, kind in kinds.items() if kind == "record")


def under(paths: list[str]) -> str:
    """An `entries` condition matching each of `paths` and every entry under it, and nothing when there are none."""
    return (
        " OR ".join(
            f"logical = {literal(path)} OR substr(logical, 1, length({literal(path + '/')})) = {literal(path + '/')}"
            for path in paths
        )
        or "0"
    )


def entries(here: Board, condition: str) -> dict[str, str]:
    """The logical path and kind of each entry in this board's own tree that matches `condition`."""
    rows = here.rows(f"SELECT logical, kind FROM entries WHERE source = 'worktree' AND ({condition})")  # noqa: S608 - values are quoted by literal()
    return {str(row[0]): str(row[1]) for row in rows}


def unresolved(here: Board, to: str, mounted: dict[str, str]) -> None:
    """Refuse each reference through a mount or link that the destination does not resolve into the same repository."""
    targets = sorted(mounted)
    answers = here.refs([to + target for target in targets])
    lost = [
        target
        for target, answer in zip(targets, answers, strict=True)
        if answer is None or answer["location"]["repository"] != mounted[target]
    ]
    refuse("share::closure::unresolved", "the destination does not resolve, into the same repository,", lost)


def read(here: Board, paths: list[str], *, local_attachments: bool) -> tuple[dict[str, Line], list[str]]:
    """Each path's batch line, a record or a text file, and the types of the records, other than Board's own."""
    listed = ", ".join(literal(path) for path in paths)
    rows = here.rows(
        "SELECT logical, kind, type, code, role FROM entries"  # noqa: S608 - values are quoted by literal()
        f" WHERE source = 'worktree' AND logical IN ({listed})",
    )
    kinds = {str(row[0]): (str(row[1]), row[2], row[4]) for row in rows}
    links = [str(row[0]) for row in rows if row[1] == "pointer"]
    refuse("share::link::unsupported", "a batch would write a copy of the target in place of each link", links)
    invalid = sorted(str(row[0]) for row in rows if refused(str(row[1]), row[3]))
    refuse("share::record::invalid", "`board put` would refuse, as this board's index does,", invalid)
    lines: dict[str, Line] = {}
    types: set[str] = set()
    unsupported: list[str] = []
    local: list[str] = []
    for path, data in zip(paths, here.get(paths), strict=True):
        if isinstance(data, PluginError):
            raise data
        kind, record_type, role = kinds[path]
        if kind == "record" and role == "markdown":
            # The file is copied as written, and its frontmatter is checked again by the destination's `put`.
            lines[path] = {"text": data.decode()}
            if isinstance(record_type, str):
                types.add(record_type)
        elif kind == "record":
            record = parsed(data)
            lines[path] = {"record": record}
            if record_type == ATTACHMENT and urlsplit(str(record.get("location"))).scheme in ("", "file"):
                local.append(path)
            elif isinstance(record_type, str) and not record_type.startswith("board/"):
                types.add(record_type)
        elif kind == "file" and utf8(data):
            lines[path] = {"text": data.decode()}
        else:
            unsupported.append(path)
    refuse("share::file::unsupported", "a batch writes only records and UTF-8 files, so it cannot copy", unsupported)
    if not local_attachments:
        refuse("share::attachment::local", "other readers cannot fetch the bytes, kept on this machine, of", local)
    return lines, sorted(types)


def refused(kind: str, code: str | None) -> bool:
    """Whether `put` refuses an entry of `kind` that the index gives `code`.

    A broken reference is admitted, as here. Any other code on a record is a
    refusal, and so is frontmatter that claims a type and does not parse,
    whose file is not a record.
    """
    if not code:
        return False
    return code.startswith("board::frontmatter::") or (kind == "record" and not code.startswith("board::ref::"))


def schema_path(record_type: str) -> str:
    """The logical path of a type's schema."""
    return f"/.board/types/{record_type}.schema"


def parsed(data: bytes | PluginError) -> dict[str, object]:
    """A record's JSON object, from bytes Board has already parsed strictly."""
    if isinstance(data, PluginError):
        raise data
    record: dict[str, object] = json.loads(data)
    return record


def utf8(data: bytes) -> bool:
    """Whether `data` is UTF-8 text, which a batch's `text` line can hold."""
    try:
        data.decode()
    except UnicodeDecodeError:
        return False
    return True


def absent(now: bytes | PluginError) -> bool:
    """Whether the destination holds no file at a path."""
    return isinstance(now, PluginError) and now.fields.get("code") in ABSENT


def same(line: Line, now: bytes | PluginError) -> bool:
    """Whether the destination holds `line` already: a record with an equal value, or a file with equal bytes."""
    if isinstance(now, PluginError):
        if absent(now):
            return False
        raise now
    if "text" in line:
        return now == str(line["text"]).encode()
    try:
        return canonical(json.loads(now)) == canonical(line["record"])
    except ValueError:
        return False


def canonical(value: object) -> str:
    """`value` as JSON with sorted keys, so `true` and `1` differ, as they do in a record."""
    return json.dumps(value, sort_keys=True)


def refuse(code: str, why: str, paths: list[str]) -> None:
    """Raise `code` naming every one of `paths`, when there are any."""
    if paths:
        named = ", ".join(paths[:10]) + (f" and {len(paths) - 10} more" if len(paths) > 10 else "")  # noqa: PLR2004
        raise PluginError(code, f"{why} {named}; nothing was written", {"paths": paths})
