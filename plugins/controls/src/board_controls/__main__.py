"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_controls.check import check
from board_controls.evaluate import evaluate
from board_controls.for_path import for_path
from board_controls.shapes import CheckOutput, EvaluateOutput, ForPathOutput


def call(request: CallRequest) -> ForPathOutput | CheckOutput | EvaluateOutput:
    """Dispatch a call to its capability."""
    if request.capability == "for-path":
        return {"controls": for_path(request)}
    if request.capability == "check":
        return check(request)
    if request.capability == "evaluate":
        return evaluate(request)
    raise PluginError(
        "controls::call::unknown_capability",
        f"controls offers no capability {request.capability}; it offers for-path, check and evaluate",
    )


sys.exit(main("controls", call))
