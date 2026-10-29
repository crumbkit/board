"""The package ships the signing vectors, so a plugin that pins it reads the same bytes Board tests against."""

import hashlib
import json
from importlib.resources import files
from typing import cast

VECTORS = files("board_plugin") / "vectors" / "signing" / "v1"


def test_the_package_holds_every_file_the_manifest_lists_with_its_sha256() -> None:
    manifest = cast("dict[str, object]", json.loads((VECTORS / "manifest.json").read_text()))
    listed = cast("dict[str, str]", manifest["files"])
    assert "order.statement" in listed
    for name, sha256 in listed.items():
        assert hashlib.sha256((VECTORS / name).read_bytes()).hexdigest() == sha256, name
