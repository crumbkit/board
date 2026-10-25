"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_loop.loop import advance, attach, next_step
from board_loop.open import open_changes
from board_loop.pr import pr
from board_loop.start import start

CAPABILITIES = {"start": start, "next": next_step, "attach": attach, "advance": advance, "open": open_changes, "pr": pr}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    capability = CAPABILITIES.get(request.capability)
    if capability is None:
        raise PluginError(
            "loop::call::unknown_capability",
            f"loop offers no capability {request.capability}; it offers {', '.join(CAPABILITIES)}",
        )
    return capability(request)


sys.exit(main("loop", call))
