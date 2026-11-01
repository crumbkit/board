"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_inflight.now import now

CAPABILITIES = {"now": now}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    capability = CAPABILITIES.get(request.capability)
    if capability is None:
        raise PluginError(
            "inflight::call::unknown_capability",
            f"inflight offers no capability {request.capability}; it offers {', '.join(CAPABILITIES)}",
        )
    return capability(request)


sys.exit(main("inflight", call))
