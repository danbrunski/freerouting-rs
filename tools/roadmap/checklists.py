#!/usr/bin/env python3
"""Roadmap checklist status and archiving (docs/roadmap/tasks).

    checklists.py status [--strict]       per-checklist [x]/[~]/[ ] counts and evidence problems
    checklists.py archive <file> [--dry-run]
                                          move a complete checklist to tasks/archive/
                                          and rewrite every link to and from it
    checklists.py self-test               run the built-in tests in a temp directory

A checklist is complete when no item is `[ ]`, every `[x]` item carries a
`*Done <date>:*` note, and every `[~]` item carries a `*Partial <date>:*` note
that names the checklist owning the remainder (an id such as `A13` or `B02.4`
from another checklist). `archive` refuses anything else. Run from anywhere;
paths are resolved from the repository root.
"""
import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Iterator

ITEM = re.compile(r"^- \[([ x~])\] \*\*([A-Z]\d{2}\.\d+)\b")
DONE = re.compile(r"\*Done \d{4}-\d{2}-\d{2}")
PARTIAL = re.compile(r"\*Partial \d{4}-\d{2}-\d{2}")
OTHER_ID = re.compile(r"\b([A-Z]\d{2})(?:\.\d+)?\b")
LINK = re.compile(r"\]\(([^)#\s]+)(#[^)]*)?\)")
CHECKLIST = re.compile(r"^[A-Z]\d{2}-.+\.md$")


def repo_root() -> pathlib.Path:
    here = pathlib.Path(__file__).resolve()
    for parent in here.parents:
        if (parent / "docs" / "roadmap").is_dir():
            return parent
    sys.exit("cannot find docs/roadmap above " + str(here))


def items(text: str) -> "Iterator[tuple[str, str, str]]":
    """Yield (box, id, body) for each checklist item; body runs to the next item or heading."""
    lines = text.splitlines()
    starts = [i for i, l in enumerate(lines) if ITEM.match(l)]
    for n, i in enumerate(starts):
        end = starts[n + 1] if n + 1 < len(starts) else len(lines)
        for j in range(i + 1, end):
            if lines[j].startswith("#"):
                end = j
                break
        m = ITEM.match(lines[i])
        yield m.group(1), m.group(2), "\n".join(lines[i:end])


def problems(path: pathlib.Path) -> tuple[dict[str, int], list[str]]:
    """(counts, list of problems that block archiving)."""
    own = path.name[:3]
    counts = {"x": 0, "~": 0, " ": 0}
    found = []
    for box, item_id, body in items(path.read_text()):
        counts[box] += 1
        if box == " ":
            found.append(f"{item_id} is not done")
        elif box == "x" and not DONE.search(body):
            found.append(f"{item_id} is [x] without a *Done <date>:* note")
        elif box == "~":
            if not PARTIAL.search(body):
                found.append(f"{item_id} is [~] without a *Partial <date>:* note")
            else:
                note = body[PARTIAL.search(body).start():]
                if not any(m.group(1) != own for m in OTHER_ID.finditer(note)):
                    found.append(f"{item_id} is [~] but its note names no other checklist owning the remainder")
    if sum(counts.values()) == 0:
        found.append("no checklist items found")
    return counts, found


def cmd_status(root: pathlib.Path, strict: bool) -> int:
    tasks = root / "docs" / "roadmap" / "tasks"
    bad = 0
    print(f"{'checklist':45} {'[x]':>4} {'[~]':>4} {'[ ]':>4}  state")
    for path in sorted(tasks.glob("*.md")):
        if not CHECKLIST.match(path.name) or "README" in path.name:
            continue
        counts, found = problems(path)
        evidence = [p for p in found if "is not done" not in p]
        state = "complete: archive it" if not found else ("evidence missing" if evidence else "open")
        if evidence:
            bad += 1
        print(f"{path.name[:45]:45} {counts['x']:>4} {counts['~']:>4} {counts[' ']:>4}  {state}")
        for p in evidence:
            print(f"    ! {p}")
    archive = tasks / "archive"
    if archive.is_dir():
        n = len([p for p in archive.glob("*.md") if CHECKLIST.match(p.name)])
        print(f"\n{n} checklist(s) in tasks/archive/")
    return 1 if (strict and bad) else 0


def rewrite_links(md: pathlib.Path, old: pathlib.Path, new: pathlib.Path, moved_self: bool,
                  old_dir_of_self: pathlib.Path) -> bool:
    """Point links in `md` at `new` instead of `old`; if `md` is the moved file, re-base its own links."""
    text = md.read_text()
    here = md.parent

    def fix(m: "re.Match[str]") -> str:
        target, anchor = m.group(1), m.group(2) or ""
        if target.startswith(("http:", "https:", "mailto:")):
            return m.group(0)
        base = old_dir_of_self if moved_self else here
        resolved = (base / target).resolve()
        if resolved == old.resolve():
            resolved = new.resolve()
        elif not moved_self:
            return m.group(0)
        rel = pathlib.Path(_relpath(resolved, here))
        return "](" + rel.as_posix() + anchor + ")"

    updated = LINK.sub(fix, text)
    if updated != text:
        md.write_text(updated)
        return True
    return False


def _relpath(target: pathlib.Path, start: pathlib.Path) -> str:
    return os.path.relpath(target, start)


def broken_links(root: pathlib.Path) -> list[str]:
    out = []
    for md in list((root / "docs" / "roadmap").rglob("*.md")):
        for m in LINK.finditer(md.read_text()):
            t = m.group(1)
            if t.startswith(("http:", "https:", "mailto:")):
                continue
            if not (md.parent / t).exists():
                out.append(f"{md.relative_to(root)} -> {t}")
    return out


def cmd_archive(root: pathlib.Path, name: str, dry: bool) -> int:
    tasks = root / "docs" / "roadmap" / "tasks"
    src = (tasks / pathlib.Path(name).name)
    if not src.exists():
        sys.exit(f"no such checklist: {src}")
    counts, found = problems(src)
    if found:
        print(f"refusing to archive {src.name}:")
        for p in found:
            print("  - " + p)
        return 1
    dst = tasks / "archive" / src.name
    print(f"archive {src.relative_to(root)} -> {dst.relative_to(root)}  ([x] {counts['x']}, [~] {counts['~']})")
    if dry:
        return 0
    dst.parent.mkdir(exist_ok=True)
    tracked = subprocess.run(["git", "-C", str(root), "ls-files", "--error-unmatch", str(src)],
                             capture_output=True).returncode == 0
    if tracked:
        subprocess.run(["git", "-C", str(root), "mv", str(src), str(dst)], check=True)
    else:
        shutil.move(str(src), str(dst))
    changed = []
    for md in (root / "docs" / "roadmap").rglob("*.md"):
        if md.resolve() == dst.resolve():
            if rewrite_links(md, src, dst, True, src.parent):
                changed.append(md)
        elif rewrite_links(md, src, dst, False, md.parent):
            changed.append(md)
    readme = next(tasks.glob(src.name[0] + "00-*README.md"), None)
    if readme is not None:
        text = readme.read_text()
        marked = re.sub(r"(\[" + src.name[:3] + r"\]\(archive/" + re.escape(src.name) + r"\))(?! ✅)", r"\1 ✅", text)
        if marked != text:
            readme.write_text(marked)
            changed.append(readme)
    for md in sorted(set(changed)):
        print("  links updated: " + str(md.relative_to(root)))
    broken = broken_links(root)
    if broken:
        print("broken links after the move:")
        for b in broken:
            print("  " + b)
        return 1
    return 0


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        tasks = root / "docs" / "roadmap" / "tasks"
        tasks.mkdir(parents=True)
        (root / "docs" / "roadmap" / "plan.md").write_text("See [T01](tasks/T01-demo.md).\n")
        (tasks / "T00-demo-README.md").write_text("| [T01](T01-demo.md) | demo |\n")
        (tasks / "T02-other.md").write_text("Needs [T01](T01-demo.md#t011).\n- [ ] **T02.1 — x.**\n")
        open_text = ("# T01\nSee [plan](../plan.md) and [T02](T02-other.md).\n\n## Checklist\n\n"
                     "- [x] **T01.1 — a.**\n  *Done 2026-10-15:* test `a` passes.\n"
                     "- [ ] **T01.2 — b.**\n")
        (tasks / "T01-demo.md").write_text(open_text)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        assert cmd_archive(root, "T01-demo.md", False) == 1, "open checklist must be refused"
        assert (tasks / "T01-demo.md").exists()
        (tasks / "T01-demo.md").write_text(open_text.replace(
            "- [ ] **T01.2 — b.**\n", "- [~] **T01.2 — b.**\n  *Partial 2026-10-15:* needs a push.\n"))
        assert cmd_archive(root, "T01-demo.md", False) == 1, "[~] without an owner must be refused"
        (tasks / "T01-demo.md").write_text(open_text.replace(
            "- [ ] **T01.2 — b.**\n", "- [~] **T01.2 — b.**\n  *Partial 2026-10-15:* the rest is T02.1.\n"))
        assert cmd_archive(root, "T01-demo.md", False) == 0, "complete checklist must archive"
        moved = (tasks / "archive" / "T01-demo.md").read_text()
        assert "](../../plan.md)" in moved and "](../T02-other.md)" in moved, moved
        assert "](tasks/archive/T01-demo.md)" in (root / "docs" / "roadmap" / "plan.md").read_text()
        assert "](archive/T01-demo.md#t011)" in (tasks / "T02-other.md").read_text()
        assert "](archive/T01-demo.md) ✅" in (tasks / "T00-demo-README.md").read_text()
        assert not broken_links(root)
    print("self-test passed")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    st = sub.add_parser("status")
    st.add_argument("--strict", action="store_true", help="exit 1 if any ticked item lacks its evidence note")
    ar = sub.add_parser("archive")
    ar.add_argument("file")
    ar.add_argument("--dry-run", action="store_true")
    sub.add_parser("self-test")
    a = ap.parse_args()
    if a.cmd == "self-test":
        return self_test()
    root = repo_root()
    if a.cmd == "status":
        return cmd_status(root, a.strict)
    return cmd_archive(root, a.file, a.dry_run)


if __name__ == "__main__":
    sys.exit(main())
