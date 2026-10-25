"""Rendering a prompt: its body, the input values it takes, and one pass of substitution (README.md, Rendering).

Every function here is pure.
"""

import re
import unicodedata
from collections.abc import Mapping

from board_plugin.protocol import PluginError

# `{name}` for an input's name, as the schema's `inputs` admits it.
PLACEHOLDER = re.compile(r"\{([a-z][a-z0-9_]{0,31})\}")
# The longest input value. A value is a short identifier, such as a pull request number; the limit keeps a
# value from carrying a second set of instructions into the prompt.
MAX_VALUE = 200
# A control character, or a Unicode line or paragraph separator, either of which would let a value span lines.
UNSAFE = re.compile(r"[\x00-\x1f\x7f-\x9f\u2028\u2029]")
# A rendered `runs_in`: a GitHub owner/name, or an absolute path or one under ~/ that never climbs out with `..`.
REPO = re.compile(r"[A-Za-z0-9-]+/(?!\.\.?$)[A-Za-z0-9._-]+")
DIRECTORY = re.compile(r"(?:/|~/)(?!(?:.*/)?\.\.(?:/|$)).*")


def unsafe(value: str) -> bool:
    """Whether `value` is too long, spans lines, or holds a control or format character such as U+202E or U+200B."""
    return len(value) > MAX_VALUE or bool(UNSAFE.search(value)) or any(unicodedata.category(c) == "Cf" for c in value)


def body(text: str) -> str:
    """The text after the line that closes the frontmatter, `---` or `...`, as Board reads a Markdown record."""
    lines = re.split(r"(?<=\n)", text)
    closing = next((i for i in range(1, len(lines)) if lines[i].rstrip("\r\n") in ("---", "...")), len(lines))
    return "".join(lines[closing + 1 :])


def checked(declared: Mapping[str, str], given: Mapping[str, str]) -> dict[str, str]:
    """The values `given`, once each names a declared input, every declared input has one, and each is one line."""
    unknown = sorted(set(given) - set(declared))
    if unknown:
        why = f"no input is named {', '.join(unknown)}; the prompt takes {', '.join(sorted(declared)) or 'none'}"
        raise PluginError("prompts::input::unknown", why, {"inputs": unknown})
    missing = sorted(set(declared) - set(given))
    if missing:
        why = f"the prompt needs a value for {', '.join(missing)}; give each in inputs on stdin"
        raise PluginError("prompts::input::missing", why, {"inputs": missing})
    bad = sorted(name for name, value in given.items() if unsafe(value))
    if bad:
        why = (
            f"the value of {', '.join(bad)} is longer than {MAX_VALUE} characters, spans lines or holds a control "
            "or format character; put a longer text in the prompt itself or in a separate brief"
        )
        raise PluginError("prompts::input::bad_value", why, {"inputs": bad})
    return dict(given)


def render(text: str, values: Mapping[str, str]) -> str:
    """`text` with each `{name}` of an input in `values` replaced by its value, in one pass; other braces stay."""
    return PLACEHOLDER.sub(lambda found: values.get(found[1], found[0]), text)


def unused(declared: Mapping[str, str], text: str) -> list[str]:
    """The declared inputs `text` never names, in the order they are declared."""
    named = {found[1] for found in PLACEHOLDER.finditer(text)}
    return [name for name in declared if name not in named]


def placed(runs_in: Mapping[str, str], values: Mapping[str, str]) -> dict[str, str]:
    """`runs_in` rendered, once the rendered value is still an owner/name or an absolute path that stays put.

    The rendered value obeys the rules for an input value too, so an override cannot carry a control or format
    character, such as a tab or U+202E, into the place the work starts.
    """
    [(key, written)] = runs_in.items()
    value = render(written, values)
    if unsafe(value) or not (REPO if key == "repo" else DIRECTORY).fullmatch(value):
        named = [found[1] for found in PLACEHOLDER.finditer(written) if found[1] in values]
        kind = "a GitHub owner/name" if key == "repo" else "an absolute path or one under ~/ without a .. segment"
        inputs = ", ".join(named) or "none"
        why = f"runs_in.{key} renders as {value!r}, which is not {kind}; check the values of the inputs: {inputs}"
        raise PluginError("prompts::runs_in::invalid", why, {"runs_in": {key: value}, "inputs": named})
    return {key: value}
