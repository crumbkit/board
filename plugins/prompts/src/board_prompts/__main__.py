"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_prompts.prompts import get, listing


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "get":
        return get(request)
    if request.capability == "list":
        return listing(request)
    raise PluginError(
        "prompts::call::unknown_capability",
        f"prompts offers no capability {request.capability}; it offers get and list",
    )


sys.exit(main("prompts", call))
