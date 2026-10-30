"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_review.covered import covered
from board_review.post import post
from board_review.run import run
from board_review.shapes import CoveredOutput, PostOutput, RunOutput


def call(request: CallRequest) -> RunOutput | PostOutput | CoveredOutput:
    """Dispatch a call to its capability."""
    if request.capability == "run":
        return run(request)
    if request.capability == "post":
        return post(request)
    if request.capability == "covered":
        return covered(request)
    raise PluginError(
        "review::call::unknown_capability",
        f"review offers no capability {request.capability}; it offers run, post and covered",
    )


sys.exit(main("review", call))
