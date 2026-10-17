"""plugin.fix_rule_areas reads and writes the DSN with open(): `Path.read_text(newline=)`
needs Python 3.13, and KiCad 10 ships 3.12 (K03.6 found the fix-up silently skipped on KiCad
10.0.7 with `TypeError: read_text() got an unexpected keyword argument 'newline'`)."""


import stub_ui
from conftest import TESTS


def test_fix_rule_areas_reads_the_dsn_without_a_python_313_api(tmp_path, monkeypatch):
    plugin, _ui = stub_ui.install(monkeypatch)
    work = tmp_path / "freerouting-rs"
    work.mkdir()
    dsn = work / "b.dsn"
    # CRLF, as KiCad writes it on Windows: the line ends must survive a read.
    text = (TESTS / "data" / "five_areas.dsn").read_text(encoding="utf-8").replace("\n", "\r\n")
    dsn.write_bytes(text.encode("utf-8"))
    before = dsn.read_bytes()
    lines = plugin.fix_rule_areas(stub_ui.FakeBoard(tmp_path / "b.kicad_pcb"), dsn, work)
    assert not any("skipped" in line for line in lines), lines
    log = (work / "plugin.log").read_text(encoding="utf-8")
    assert "WARNING: rule-area fix-up skipped" not in log
    assert dsn.read_bytes() == before  # the fake board has no rule areas: nothing to rewrite
