import hashlib
import json
import sys

import pytest
from conftest import PLUGIN_SRC

sys.path.insert(0, str(PLUGIN_SRC.parent))
import install  # noqa: E402


def tree_digest(root):
    h = hashlib.sha256()
    for p in sorted(root.rglob("*")):
        h.update(str(p.relative_to(root)).encode())
        if p.is_file():
            h.update(p.read_bytes())
    return h.hexdigest()


def make_plugin(folder, identifier=None):
    folder.mkdir(parents=True)
    (folder / "plugin.py").write_text("# stub\n")
    if identifier:
        (folder / "metadata.json").write_text(json.dumps({"identifier": identifier}))
    return folder


@pytest.fixture
def kicad(tmp_path):
    """<tmp>/KiCad/10.0 with scripting/plugins, 3rdparty/plugins and the Java plugin."""
    version_dir = tmp_path / "KiCad" / "10.0"
    plugins = version_dir / "scripting" / "plugins"
    plugins.mkdir(parents=True)
    java = make_plugin(version_dir / "3rdparty" / "plugins" / "app_freerouting_kicad-plugin", install.JAVA_IDENTIFIER)
    return version_dir, plugins, java


def run(plugins, *extra):
    install.main(["--plugins-dir", str(plugins), *extra])


def test_shadow_copy_with_our_identifier_is_moved_and_reported(kicad, capsys):
    version_dir, plugins, java = kicad
    ident = sorted(install.OUR_IDENTIFIERS)[0]
    shadow = make_plugin(version_dir / "3rdparty" / "plugins" / ident.replace(".", "_"), ident)
    before_java = tree_digest(java)

    run(plugins)

    moved = version_dir.parent / "disabled_pcm_plugins" / "10.0" / shadow.name
    assert not shadow.exists() and (moved / "plugin.py").is_file()
    out = capsys.readouterr().out
    assert str(shadow) in out and str(moved) in out
    assert tree_digest(java) == before_java
    assert (plugins / "freerouting_rs" / "tools.py").is_file()
    assert (plugins / "freerouting_rs" / "version.py").is_file()


def test_java_plugin_alone_is_left_untouched(kicad, capsys):
    version_dir, plugins, java = kicad
    before = tree_digest(java)
    run(plugins)
    assert tree_digest(java) == before
    assert not (version_dir.parent / "disabled_pcm_plugins").exists()
    assert "Disabled" not in capsys.readouterr().out


def test_java_identifier_is_never_ours_even_under_our_folder_name(kicad):
    version_dir, plugins, java = kicad
    odd = make_plugin(version_dir / "3rdparty" / "plugins" / "freerouting_rs", install.JAVA_IDENTIFIER)
    before = tree_digest(odd)
    run(plugins)
    assert tree_digest(odd) == before


def test_shadow_found_by_folder_name_and_in_scripting_plugins_by_identifier(kicad):
    version_dir, plugins, _ = kicad
    by_name = make_plugin(version_dir / "3rdparty" / "plugins" / "freerouting_rs")
    ident = sorted(install.OUR_IDENTIFIERS)[0]
    by_ident = make_plugin(plugins / "renamed_copy", ident)
    unrelated = make_plugin(plugins / "some_other_plugin", "com.example.other")
    before = tree_digest(unrelated)
    run(plugins)
    assert not by_name.exists() and not by_ident.exists()
    assert tree_digest(unrelated) == before
    assert (plugins / "freerouting_rs").is_dir()


def test_keep_pcm_refuses_and_changes_nothing(kicad):
    version_dir, plugins, java = kicad
    ident = sorted(install.OUR_IDENTIFIERS)[0]
    shadow = make_plugin(version_dir / "3rdparty" / "plugins" / ident.replace(".", "_"), ident)
    before = tree_digest(version_dir)
    with pytest.raises(SystemExit) as e:
        run(plugins, "--keep-pcm")
    assert str(shadow) in str(e.value)
    assert tree_digest(version_dir) == before
    assert not (plugins / "freerouting_rs").exists()


def test_colliding_disabled_name_gets_a_suffix(kicad):
    version_dir, plugins, _ = kicad
    ident = sorted(install.OUR_IDENTIFIERS)[0]
    name = ident.replace(".", "_")
    for _ in range(2):
        make_plugin(version_dir / "3rdparty" / "plugins" / name, ident)
        run(plugins)
    disabled = version_dir.parent / "disabled_pcm_plugins" / "10.0"
    assert sorted(p.name for p in disabled.iterdir()) == [name, name + "-1"]


def test_reinstall_over_own_folder_is_not_a_shadow(kicad):
    version_dir, plugins, _ = kicad
    run(plugins)
    run(plugins)
    assert not (version_dir.parent / "disabled_pcm_plugins").exists()
    assert (plugins / "freerouting_rs" / "plugin.py").is_file()
