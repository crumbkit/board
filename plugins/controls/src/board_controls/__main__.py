"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main
from pydantic import JsonValue

from board_controls.check import check
from board_controls.for_path import for_path


def call(request: CallRequest) -> JsonValue:
    """Dispatch a call to its capability."""
    if request.capability == "for-path":
        return for_path(request).model_dump(mode="json")
    if request.capability == "check":
        return check(request).model_dump(mode="json")
    raise PluginError(
        "controls::call::unknown_capability",
        f"controls offers no capability {request.capability}; it offers for-path and check",
    )


sys.exit(main("controls", call))
