"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_share.to import share_to


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "to":
        return share_to(request)
    raise PluginError(
        "share::call::unknown_capability", f"share offers no capability {request.capability}; it offers to"
    )


sys.exit(main("share", call))
