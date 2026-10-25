"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_plans.open import open_questions


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "open":
        return open_questions(request)
    raise PluginError(
        "plans::call::unknown_capability", f"plans offers no capability {request.capability}; it offers open"
    )


sys.exit(main("plans", call))
