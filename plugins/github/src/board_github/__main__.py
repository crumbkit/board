"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_github.ref import ref

CAPABILITIES = {"ref": ref}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    capability = CAPABILITIES.get(request.capability)
    if capability is None:
        raise PluginError(
            "github::call::unknown_capability",
            f"github offers no capability {request.capability}; it offers {', '.join(CAPABILITIES)}",
        )
    return capability(request)


sys.exit(main("github", call))
