"""pcbnew is not thread-safe: a worker-thread SaveBoard froze KiCad 10.0.5 on
Windows 11 (KiCadRoutingTools issue #688). Worker threads may only run and
read the subprocess. This test reads plugin.py with `ast`, finds every
function passed as `threading.Thread(target=...)` (a name, `self.method` or a
lambda), follows the calls it makes inside the package, and fails if any
path reaches `pcbnew.*` or `wx.*` other than `wx.CallAfter`."""

import ast
import shutil

from conftest import PLUGIN_SRC

FORBIDDEN_ROOTS = {"pcbnew", "wx"}
ALLOWED = {("wx", "CallAfter")}


def load_package(directory):
    return {p.stem: ast.parse(p.read_text(encoding="utf-8"), str(p)) for p in directory.glob("*.py")}


def index_functions(tree):
    funcs = {}
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            funcs.setdefault(node.name, []).append(node)
    return funcs


def chain(node):
    """['wx', 'CallAfter'] for wx.CallAfter(...), else None."""
    parts = []
    while isinstance(node, ast.Attribute):
        parts.append(node.attr)
        node = node.value
    if isinstance(node, ast.Name):
        return [node.id] + parts[::-1]
    return None


def thread_targets(tree):
    out = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and chain(node.func) in (["threading", "Thread"], ["Thread"]):
            for kw in node.keywords:
                if kw.arg == "target":
                    out.append(kw.value)
    return out


def violations(package, root_module="plugin"):
    """List of (module, line, text) for forbidden references reachable from
    worker-thread targets, and the number of targets found."""
    funcs = {m: index_functions(t) for m, t in package.items()}
    found, seen = [], set()

    def visit(module, node):
        subs = list(ast.walk(node))
        inner = {id(n.value) for n in subs if isinstance(n, ast.Attribute)}  # report outermost chains only
        for sub in subs:
            is_ref = isinstance(sub, (ast.Attribute, ast.Name)) and id(sub) not in inner
            ch = chain(sub) if is_ref else None
            if ch and ch[0] in FORBIDDEN_ROOTS and tuple(ch[:2]) not in ALLOWED:
                found.append((module, sub.lineno, ".".join(ch)))
            if isinstance(sub, ast.Call):
                follow(module, sub.func)

    def follow(module, func):
        ch = chain(func)
        if not ch:
            return
        if len(ch) == 1:
            targets = [(module, f) for f in funcs[module].get(ch[0], [])]
        elif ch[0] == "self" and len(ch) == 2:
            targets = [(module, f) for f in funcs[module].get(ch[1], [])]
        elif ch[0] in package and len(ch) == 2:
            targets = [(ch[0], f) for f in funcs[ch[0]].get(ch[1], [])]
        else:
            targets = []
        for mod, f in targets:
            if id(f) not in seen:
                seen.add(id(f))
                visit(mod, f)

    roots = thread_targets(package[root_module])
    for target in roots:
        if isinstance(target, ast.Lambda):
            visit(root_module, target)
        else:
            visit(root_module, ast.Expression(body=target))
            follow(root_module, target)
    return found, len(roots)


def test_routing_worker_never_touches_pcbnew_or_wx():
    found, n_targets = violations(load_package(PLUGIN_SRC))
    assert n_targets >= 1, "no threading.Thread(target=...) found; the guard would pass vacuously"
    assert found == []


def plant(tmp_path, old, new):
    copy = tmp_path / "freerouting_rs"
    shutil.copytree(PLUGIN_SRC, copy, ignore=shutil.ignore_patterns("__pycache__"))
    src = (copy / "plugin.py").read_text(encoding="utf-8")
    assert old in src
    (copy / "plugin.py").write_text(src.replace(old, new), encoding="utf-8")
    return copy


def test_negative_control_lambda_worker_calling_pcbnew(tmp_path):
    copy = plant(tmp_path, "result.update(r=tools.finish_route(self.router, proc))", "pcbnew.Refresh()")
    found, _ = violations(load_package(copy))
    assert [f[2] for f in found] == ["pcbnew.Refresh"]


def test_negative_control_reaches_through_helpers_and_tools(tmp_path):
    copy = plant(tmp_path, "tools.finish_route(self.router, proc)", "self.helper()")
    src = (copy / "plugin.py").read_text(encoding="utf-8")
    src = src.replace("    def on_view(self, _evt):", "    def helper(self):\n        return tools.sneaky()\n\n    def on_view(self, _evt):")
    (copy / "plugin.py").write_text(src, encoding="utf-8")
    with open(copy / "tools.py", "a", encoding="utf-8") as f:
        f.write("\n\ndef sneaky():\n    return wx.Yield()\n")
    found, _ = violations(load_package(copy))
    assert [f[2] for f in found] == ["wx.Yield"]


def test_wx_callafter_is_allowed(tmp_path):
    copy = plant(tmp_path, "result.update(r=tools.finish_route(self.router, proc))", "wx.CallAfter(print, 1)")
    assert violations(load_package(copy))[0] == []
