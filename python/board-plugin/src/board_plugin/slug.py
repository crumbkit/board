"""The file name Reflect gives a note of a title, which notes and tasks name new files by."""

import re
import unicodedata

# Reflect's slugForTitle (packages/core/src/markdown/slug.ts), whose golden corpus freezes it.
SLUG_CHARS = 60
# JavaScript's `\s`, which differs from Python's: it holds U+FEFF, and not U+001C to U+001F or U+0085.
SEPARATOR = re.compile("[\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff_-]+")
RESERVED = {"con", "prn", "aux", "nul", *(f"com{n}" for n in range(1, 10)), *(f"lpt{n}" for n in range(1, 10))}


def slug(title: str) -> str:
    """The file name Reflect gives a note titled `title`, without `.md`.

    It drops a letter or number newer than this Python's Unicode database
    knows, where Reflect's JavaScript, on a newer database, keeps it.
    """
    folded = unicodedata.normalize("NFC", title).lower()
    kept = "".join(c for c in folded if unicodedata.category(c)[0] in "LN" or SEPARATOR.match(c))
    capped = SEPARATOR.sub("-", kept).strip("-")[:SLUG_CHARS].strip("-")
    if not capped:
        return "untitled"
    return f"{capped}-note" if capped in RESERVED else capped
