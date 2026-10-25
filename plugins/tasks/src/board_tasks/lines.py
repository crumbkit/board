"""The line editor the write calls change a task's frontmatter with (README.md, Writing tasks).

It replaces or adds single top-level `key: value` lines and leaves every other byte as it was, so a
comment, a key order or a field another tool wrote survives. It reads lines, never YAML, so it refuses a
key whose line is not one plain scalar rather than guess how to rewrite it.
"""

import json
import re

from board_plugin.protocol import PluginError

# The values a line holds on its own: a double- or single-quoted string closed on the line, or a plain
# scalar that starts with none of YAML's indicators of a block, a collection, an anchor or a tag; then an
# optional comment.
ONE_LINE = re.compile(r"""("(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[^\s|>\[{&*!"'#][^#]*?)\s*(?:\s#.*)?""")


def with_fields(text: str, values: dict[str, str], path: str) -> str:
    """`text` with each key's line set to its value, written as JSON, which is YAML; a missing key goes last."""
    lines = re.split(r"(?<=\n)", text)
    newline = "\r\n" if lines[0].endswith("\r\n") else "\n"
    if lines[0].removeprefix("﻿").rstrip("\r\n") != "---":
        raise PluginError("tasks::task::not_plain", f"{path} has no frontmatter; start it with ---", {"path": path})
    end = next((i for i in range(1, len(lines)) if lines[i].rstrip("\r\n") == "---"), None)
    if end is None:
        why = f"{path} has no line --- closing its frontmatter; add one after its last field"
        raise PluginError("tasks::task::not_plain", why, {"path": path})
    added: list[str] = []
    for key, value in values.items():
        line = f"{key}: {json.dumps(value, ensure_ascii=False)}{newline}"
        found = [i for i in range(1, end) if re.match(rf"{re.escape(key)}\s*:(\s|$)", lines[i])]
        if not found:
            added.append(line)
            continue
        at = found[0]
        held = lines[at].split(":", 1)[1].strip()
        continued = at + 1 < end and lines[at + 1][:1] in (" ", "\t")
        if len(found) > 1 or continued or not ONE_LINE.fullmatch(held):
            shape = "with no value" if not held and not continued else "as a block, a list or a tag"
            why = (
                f"{path} holds {key} {'twice' if len(found) > 1 else shape}, and this call rewrites only one line "
                f"holding one plain value; make it one line such as `{key}: value`, then call again"
            )
            raise PluginError("tasks::task::not_plain", why, {"path": path, "key": key})
        lines[at] = line
    return "".join(lines[:end] + added + lines[end:])
