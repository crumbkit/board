"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_runs.latest import latest
from board_runs.trend import trend


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "latest":
        return latest(request).model_dump(mode="json")
    if request.capability == "trend":
        return trend(request).model_dump(mode="json")
    raise PluginError(
        "runs::call::unknown_capability",
        f"runs offers no capability {request.capability}; it offers latest and trend",
    )


sys.exit(main("runs", call))
