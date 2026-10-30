"""The `get` and `list` capabilities (README.md). Both only read, and neither syncs a repo."""

import json
from concurrent.futures import ThreadPoolExecutor
from typing import Any

from board_plugin import repos
from board_plugin.board import Board, literal
from board_plugin.protocol import CallRequest, PluginError

from board_prompts.render import body, checked, placed, render, unused

PROMPT_TYPE = "prompts/prompt/v1"
REPOS_TYPE = "prompts/repos/v1"
# The list of marked repos, in the board's own working tree.
MARKS = "prompts/repos.json"
# Codes that depend on the tree rather than the record, such as a link in the body to a file not there yet.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")
# The most prompts `list` shows; `total` counts the rest (STANDARDS 23).
SHOWN = 100
# Each mount whose pointer is in the board's own working tree, never inside another mount: its root, the commit
# it pins, the clone Board read that commit from, and its `notes-repo.json` record's type, status, code and body.
MOUNTS = f"""
SELECT 'mount', '/' || substr(m.path, 1, length(m.path) - 5), m.target, s.repository, e.type, e.status, e.code, r.body
FROM mounts AS m
LEFT JOIN sources AS s ON s.source = m.target
LEFT JOIN entries AS e ON e.source = m.target AND e.path = {literal(repos.RECORD)}
LEFT JOIN records AS r ON r.source = m.target AND r.path = {literal(repos.RECORD)}
WHERE m.source = 'worktree' AND m.code IS NULL AND m.target IS NOT NULL
"""  # noqa: S608 - the only values interpolated are constants, quoted by `literal`
# The list of marked repos: its type, status, code and body.
MARKED = f"""
SELECT 'marks', e.type, e.status, e.code, r.body, NULL, NULL, NULL
FROM entries AS e LEFT JOIN records AS r ON r.logical = e.logical
WHERE e.source = 'worktree' AND e.path = {literal(MARKS)}
"""  # noqa: S608 - the only values interpolated are constants, quoted by `literal`
# Each Markdown file under a `prompts/` directory of its repo: the commit it was read from, the root of the
# path Board indexed it at, its logical path, its path in the repo, its type, Board code and frontmatter.
COLUMNS = (
    "e.source",
    "substr(e.logical, 1, length(e.logical) - length(e.path))",
    "e.logical",
    "e.path",
    "e.type",
    "e.code",
    "r.body",
)


def files(where: str) -> str:
    """The mounts, the marks and the files `where` selects, in one query, since each `board query` costs a run."""
    return f"""
{MOUNTS}
UNION ALL
{MARKED}
UNION ALL
SELECT * FROM (
  SELECT 'file', {", ".join(COLUMNS)} FROM entries AS e LEFT JOIN records AS r ON r.logical = e.logical
  WHERE {where} ORDER BY e.logical
)
"""  # noqa: S608 - `where` is built from constants and values quoted by `literal`


def query(board: Board, where: str) -> tuple[list[repos.Listed], dict[str, str], list[list[Any]]]:
    """The marked repos, the root of each by the commit it pins, and the rows of the files `where` selects.

    A repo is marked when `/prompts/repos.json` in the board's own tree names it and its mount pointer is in
    that same tree. Nothing a mounted repository commits, such as a `daily/` directory, a `notes-repo.json` or a
    mount pointer of its own, can mark it. `worktree` is the board's own tree, `/`.
    """
    rows = board.rows(files(where))
    marked = marks(board.plugin, [row[1:5] for row in rows if row[0] == "marks"])
    mounts = [row[1:] for row in rows if row[0] == "mount" and repos.root(str(row[1])) in marked]
    found = repos.parsed(board.plugin, mounts)
    roots = {"worktree": "/", **{str(source): at for at, source, _, _ in found}}
    return found, roots, [row[1 : 1 + len(COLUMNS)] for row in rows if row[0] == "file"]


def marks(plugin: str, rows: list[list[Any]]) -> set[str]:
    """The roots `/prompts/repos.json` names; none without the file, and refused when it is not valid."""
    if not rows:
        return set()
    [[kind, status, code, body]] = rows
    if kind != REPOS_TYPE or status != "ok":
        why = (
            f"/{MARKS} is not a valid {REPOS_TYPE} record (type {kind}, status {status}, code {code}), so no "
            f"mount's prompts are admitted; run `board check /{MARKS}` to see what is wrong"
        )
        raise PluginError(f"{plugin}::repos::invalid", why, {"path": f"/{MARKS}", "code": code})
    return {repos.root(str(at)) for at in json.loads(body)["mounts"]}


def get(request: CallRequest) -> dict[str, object]:
    """Read one prompt at its pin and render it with the input values given."""
    board = Board(request.board.root, "prompts")
    path = str(request.input["path"])
    file = repos.Repo(board, "/", None).read(path.lstrip("/"))
    if file.blob is None or file.location is None:
        raise PluginError("prompts::prompt::not_found", f"nothing is at {path}; run `board ls` to see what is there")
    source, inner = str(file.location["source"]), str(file.location["path"])
    found, roots, rows = query(board, f"e.source = {literal(source)} AND e.path = {literal(inner)}")
    if not rows:
        raise PluginError("prompts::board::refused", f"board get read {path}, but board query has no entry for it")
    _, at, _, _, kind, code, _ = rows[0]
    repo = roots.get(source)
    if repo is None:
        held = repos.root(str(at))
        why = (
            f"{path} is in {held}, which the board does not mark, and prompts come only from the board's own tree "
            f"and the mounts /{MARKS} names; move the prompt, or add the mount's path to /{MARKS} if its pointer "
            "is in the board's own tree"
        )
        raise PluginError("prompts::prompt::outside_scope", why, {"path": path, "repo": held})
    if kind == PROMPT_TYPE and code == "board::record::unknown_type":
        raise no_schema(board, repo, next(str(clone) for at, _, clone, _ in found if at == repo))
    if "prompts" not in inner.split("/")[:-1] or kind != PROMPT_TYPE or code not in (None, *TREE_CODES):
        why = (
            f"{path} is not a valid {PROMPT_TYPE} under a prompts/ directory (type {kind}, code {code}); "
            f"run `board check {path}` to see what is wrong"
        )
        raise PluginError("prompts::prompt::not_prompt", why, {"path": path, "code": code})
    record: dict[str, Any] = file.record or {}
    declared: dict[str, str] = record.get("inputs") or {}
    given: dict[str, str] = request.input.get("inputs") or {}  # type: ignore[assignment]
    values = checked(declared, given)
    text = body(file.text or "")
    return {
        "path": path,
        "repo": repo,
        "title": record["title"],
        "purpose": record["purpose"],
        "runs_in": placed(request.input.get("runs_in") or record["runs_in"], values),  # type: ignore[arg-type]
        "text": render(text, values),
        "blob": file.blob,
        "source": source,
        "unused_inputs": unused(declared, text),
    }


def no_schema(board: Board, repo: str, clone: str) -> PluginError:
    """The refusal of a prompt whose repo lacks the prompt schema, with the commands that install it."""
    why = (
        f"{repo} holds no schema for {PROMPT_TYPE}, so Board cannot check its prompts; run "
        f"`board -C {clone} plugin enable prompts`, push, then `board -C {board.root} mount update {repo}`"
    )
    return PluginError("prompts::repo::no_schema", why, {"repo": repo, "type": PROMPT_TYPE})


def listing(request: CallRequest) -> dict[str, object]:
    """The prompts in the repos in scope, read at their pins, by repo then title."""
    board = Board(request.board.root, "prompts")
    name, given = request.input.get("repo"), request.input.get("remote")
    with ThreadPoolExecutor() as pool:
        origin = pool.submit(repos.caller_origin) if name is None and given is None else None
        where = f"e.type = {literal(PROMPT_TYPE)} AND e.path GLOB '*.md' AND ('/' || e.path) GLOB '*/prompts/*'"
        found, held, rows = query(board, where)
        remote, unread = (None if given is None else str(given), None) if origin is None else origin.result()
    roots = repos.scope(board.plugin, found, None if name is None else str(name), remote, unread)
    prompts: list[dict[str, Any]] = []
    degraded = 0
    for source, _, logical, _, _, code, frontmatter in rows:
        repo = held.get(str(source))
        if repo not in roots:
            continue
        if code not in (None, *TREE_CODES) or frontmatter is None:
            degraded += 1
            continue
        record = json.loads(frontmatter)
        prompts.append(
            {
                "repo": repo,
                "path": logical,
                "title": record["title"],
                "purpose": record["purpose"],
                "runs_in": record["runs_in"],
                "inputs": record.get("inputs") or {},
            }
        )
    prompts.sort(key=lambda prompt: (prompt["repo"], prompt["title"], prompt["path"]))
    return {
        "prompts": prompts[:SHOWN],
        "total": len(prompts),
        "degraded": degraded,
        "repos": roots,
        "origin_unread": unread,
    }
