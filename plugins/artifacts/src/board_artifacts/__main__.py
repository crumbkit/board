"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_artifacts.importing import import_listing
from board_artifacts.listed import listed


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "list":
        return listed(request).model_dump(mode="json")
    if request.capability == "import":
        return import_listing(request).model_dump(mode="json")
    raise PluginError(
        "artifacts::call::unknown_capability",
        f"artifacts offers no capability {request.capability}; it offers list and import",
    )


sys.exit(main("artifacts", call))
