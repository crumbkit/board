"""The committed manifest and schema are the generated ones, and enabling the plugin installs the schema."""

import json

from board_plugin.protocol import dumped
from conftest import PLUGIN, Board, control

from board_controls.manifest import manifest, schema


def test_the_committed_manifest_and_schema_are_the_ones_the_models_generate() -> None:
    assert (PLUGIN / "plugin.json").read_text() == dumped(manifest())
    assert (PLUGIN / "types/controls/control/v1.schema.json").read_text() == dumped(schema())


def test_enabling_writes_the_schema_so_a_control_checks_clean_and_a_broken_one_does_not(board: Board) -> None:
    board.write("controls/ok.json", control("R."))
    board.write("controls/bad.json", '{"$type": "controls/control/v1", "requirement": "R."}')
    report = json.loads(board.run("--json", "check").stdout)
    status = {entry["logical"]: entry for entry in report["entries"]}
    assert status["/controls/ok"]["status"] == "ok"
    assert status["/.board/types/controls/control/v1.schema"]["status"] == "ok"
    assert status["/controls/bad"]["status"] != "ok"
    assert status["/controls/bad"]["code"] == "board::record::invalid"
