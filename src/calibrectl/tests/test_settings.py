from pathlib import Path

import pytest

from calibrectl import settings


def test_parse_values_expansion_and_errors():
    s = settings.parse("version: 1\ndatabase: ${HOME}/lib/metadata.db\nattach_roots: [\"${HOME}/share\", /srv/in]\n", "f", {"HOME": "/h"})
    assert s == settings.Settings(database=Path("/h/lib/metadata.db"), storage=None, attach_roots=(Path("/h/share"), Path("/srv/in")))
    assert settings.parse("", "f", {}) == settings.Settings()
    with pytest.raises(ValueError, match="colour"):
        settings.parse("colour: red\n", "f", {})
    with pytest.raises(ValueError, match="attach_roots"):
        settings.parse("attach_roots: /one\n", "f", {})
    with pytest.raises(ValueError, match="NOPE"):
        settings.parse("database: ${NOPE}/x\n", "f", {})
    with pytest.raises(ValueError, match="the-file"):
        settings.parse("database: [\n", "the-file", {})


def test_load_reads_the_xdg_file(tmp_path):
    (tmp_path / "rafa").mkdir()
    (tmp_path / "rafa/calibrectl.yaml").write_text("storage: /lib\n")
    assert settings.load({"XDG_CONFIG_HOME": str(tmp_path)}).storage == Path("/lib")
    assert settings.load({"HOME": str(tmp_path / "nobody")}) == settings.Settings()
