"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main
from pydantic import JsonValue

from board_evals.about import about
from board_evals.open import open_evals


def call(request: CallRequest) -> JsonValue:
    """Dispatch a call to its capability."""
    if request.capability == "about":
        return about(request).model_dump(mode="json")
    if request.capability == "open":
        return open_evals(request).model_dump(mode="json")
    raise PluginError(
        "evals::call::unknown_capability",
        f"evals offers no capability {request.capability}; it offers about and open",
    )


sys.exit(main("evals", call))
