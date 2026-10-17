"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_controls.check import check
from board_controls.for_path import for_path
from board_controls.shapes import CheckOutput, ForPathOutput


def call(request: CallRequest) -> ForPathOutput | CheckOutput:
    """Dispatch a call to its capability."""
    if request.capability == "for-path":
        return {"controls": for_path(request)}
    if request.capability == "check":
        return check(request)
    raise PluginError(
        "controls::call::unknown_capability",
        f"controls offers no capability {request.capability}; it offers for-path and check",
    )


sys.exit(main("controls", call))
