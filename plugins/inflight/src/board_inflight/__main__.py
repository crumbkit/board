"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_inflight.now import now


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "now":
        return now(request)
    raise PluginError(
        "inflight::call::unknown_capability",
        f"inflight offers no capability {request.capability}; it offers now",
    )


sys.exit(main("inflight", call))
