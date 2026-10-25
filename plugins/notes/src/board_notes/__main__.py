"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_notes.notes import add, daily


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "add":
        return add(request)
    if request.capability == "daily":
        return daily(request)
    raise PluginError(
        "notes::call::unknown_capability", f"notes offers no capability {request.capability}; it offers add and daily"
    )


sys.exit(main("notes", call))
