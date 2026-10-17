#!/usr/bin/env bash
# Sets up KiCadRoutingTools for X01 in its own environment, never writing into the KRT repo.
#
#   tools/krt-h2h/setup-krt.sh [--reexport]
#
# 1. records `git rev-parse HEAD` and `status --porcelain` of $KRT_REPO (before);
# 2. exports $KRT_PIN with `git archive` into $X01_DIR/krt (kept if present, unless --reexport);
# 3. creates $X01_DIR/venv (no system site-packages, so no pcbnew), installs the export's
#    requirements.txt and records `pip freeze`;
# 4. builds grid_router with `build_router.py --from-source` inside the export; records
#    `rustc --version` and the sha256 of grid_router.so;
# 5. smoke test: routes kicad_files/cap_chain.kicad_pcb with route.py in the export, with no
#    kicad-cli reachable; checks exit 0 and an output board;
# 6. routes SHIM_BOARD (default kicad_files/esp_prog.kicad_pcb: small, with zones, which is
#    where KRT calls KiCad; cap_chain has none) with kicad-cli-shim.sh as kicad-cli and records
#    whether KRT called KiCad and whether those calls succeeded;
# 7. records HEAD and status again (after); they must equal (1).
# Everything recorded goes to $X01_DIR/krt-env.json. Exit 0 only if (7) holds and (5) passed.
set -uo pipefail
source "$(dirname "$0")/common.sh"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
unset CARGO_TARGET_DIR  # build_router.py expects rust_router/target

REEXPORT=""
[[ "${1:-}" == "--reexport" ]] && REEXPORT=1
mkdir -p "$X01_DIR/setup"
LOGS="$X01_DIR/setup"

before_head=$(git -C "$KRT_REPO" rev-parse HEAD)
git -C "$KRT_REPO" status --porcelain > "$LOGS/status-before.txt"

if [[ -n "$REEXPORT" ]]; then
  rm -rf "$KRT_EXPORT"
fi
if [[ ! -d "$KRT_EXPORT" ]]; then
  mkdir -p "$KRT_EXPORT"
  git -C "$KRT_REPO" archive "$KRT_PIN" | tar -x -C "$KRT_EXPORT" || {
    echo "git archive of $KRT_PIN failed" >&2
    exit 1
  }
fi

if [[ ! -x "$KRT_VENV/bin/python" ]]; then
  python3 -m venv "$KRT_VENV" || exit 1
fi
"$KRT_VENV/bin/python" -m pip install --disable-pip-version-check -q \
  -r "$KRT_EXPORT/requirements.txt" > "$LOGS/pip-install.log" 2>&1 ||
  { echo "pip install failed (see $LOGS/pip-install.log)" >&2; exit 1; }
"$KRT_VENV/bin/python" -m pip freeze --disable-pip-version-check > "$LOGS/pip-freeze.txt"

# An export under another cargo workspace (X01_DIR inside this repository's target/) would
# make cargo treat rust_router as a member of that workspace; an empty [workspace] table in the
# export's manifest keeps it standalone. Build-only, recorded in krt-env.json as export_patch.
EXPORT_PATCH=""
if ! grep -q '^\[workspace\]' "$KRT_EXPORT/rust_router/Cargo.toml"; then
  dir=$(dirname "$KRT_EXPORT")
  while [[ "$dir" != / ]]; do
    if [[ -f "$dir/Cargo.toml" ]] && grep -q '^\[workspace\]' "$dir/Cargo.toml"; then
      printf '\n[workspace]\n' >> "$KRT_EXPORT/rust_router/Cargo.toml"
      EXPORT_PATCH="appended an empty [workspace] table to rust_router/Cargo.toml (the export lies under the cargo workspace $dir)"
      break
    fi
    dir=$(dirname "$dir")
  done
elif [[ -f "$X01_DIR/setup/export-patch.txt" ]]; then
  EXPORT_PATCH=$(cat "$X01_DIR/setup/export-patch.txt")
fi
printf '%s' "$EXPORT_PATCH" > "$X01_DIR/setup/export-patch.txt"
(cd "$KRT_EXPORT" && "$KRT_VENV/bin/python" -I build_router.py --from-source) \
  > "$LOGS/build-router.log" 2>&1 || { echo "build_router.py failed (see $LOGS/build-router.log)" >&2; exit 1; }
SO=$(find "$KRT_EXPORT/rust_router" -maxdepth 1 -name 'grid_router*.so' | sort | head -n 1)
[[ -n "$SO" ]] || { echo "no grid_router.so after the build" >&2; exit 1; }

SHIM_BOARD=${SHIM_BOARD:-kicad_files/esp_prog.kicad_pcb}
SHIM_BASE=$(basename "$SHIM_BOARD" .kicad_pcb)
KRT_PATH=$(no_kicad_path) || exit 1
SMOKE="$X01_DIR/smoke"
rm -rf "$SMOKE"
mkdir -p "$SMOKE/pure" "$SMOKE/shim"
cp "$KRT_EXPORT/kicad_files/cap_chain.kicad_pcb" "$SMOKE/pure/"
for f in "$KRT_EXPORT"/kicad_files/cap_chain.kicad_{pro,dru}; do
  [[ -f "$f" ]] && cp "$f" "$SMOKE/pure/"
done
for f in "$KRT_EXPORT/${SHIM_BOARD%.kicad_pcb}".kicad_{pcb,pro,dru}; do
  [[ -f "$f" ]] && cp "$f" "$SMOKE/shim/"
done

smoke() { # dir path_value board_base
  (cd "$KRT_EXPORT" && env -u KICAD_CLI PATH="$2" KRT_SHIM_LOG="$1/shim.log" timeout 1800 \
    "$KRT_VENV/bin/python" -E -s py_router/route.py "$1/$3.kicad_pcb" \
    "$1/$3.routed.kicad_pcb" --nets '*' --fab-tier standard --escalation off \
    --same-net-pad-clearance 0.1 --no-fix-drc-settings --keep-input-copper) > "$1/route.log" 2>&1
}
smoke "$SMOKE/pure" "$KRT_PATH" cap_chain
PURE_EXIT=$?
SHIM_PATH=$(shim_path) || exit 1
touch "$SMOKE/shim/shim.log"
smoke "$SMOKE/shim" "$SHIM_PATH" "$SHIM_BASE"
SHIM_EXIT=$?

after_head=$(git -C "$KRT_REPO" rev-parse HEAD)
git -C "$KRT_REPO" status --porcelain > "$LOGS/status-after.txt"

python3 - "$X01_DIR/krt-env.json" "$before_head" "$after_head" "$LOGS" "$SO" "$SMOKE" \
  "$PURE_EXIT" "$SHIM_EXIT" "$KRT_PIN" "$KRT_EXPORT" "$KRT_PATH" \
  "$("$KRT_VENV/bin/python" --version 2>&1)" "$SHIM_BOARD" << 'PY'
import hashlib, json, os, subprocess, sys
(out, before, after, logs, so, smoke, pure_exit, shim_exit, pin, export, krt_path,
 venv_python, shim_board) = sys.argv[1:14]
shim_base = os.path.basename(shim_board)[:-len(".kicad_pcb")]
read = lambda p: open(p, encoding="utf-8").read()
sha = lambda p: hashlib.sha256(open(p, "rb").read()).hexdigest()
shim_calls = [l for l in read(os.path.join(smoke, "shim", "shim.log")).splitlines() if l]
pure_out = os.path.join(smoke, "pure", "cap_chain.routed.kicad_pcb")
shim_out = os.path.join(smoke, "shim", shim_base + ".routed.kicad_pcb")
def grep(path, *words):
    return [l.strip() for l in read(path).splitlines() if any(w in l for w in words)][:20]
env = {
    "krt_pin": pin,
    "krt_export": export,
    "repo_head_before": before,
    "repo_head_after": after,
    "repo_status_before": read(os.path.join(logs, "status-before.txt")),
    "repo_status_after": read(os.path.join(logs, "status-after.txt")),
    "repo_unchanged": before == after and read(os.path.join(logs, "status-before.txt"))
        == read(os.path.join(logs, "status-after.txt")),
    "python": venv_python,
    "pip_freeze": read(os.path.join(logs, "pip-freeze.txt")).splitlines(),
    "rustc": subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip(),
    "cargo": subprocess.run(["cargo", "--version"], capture_output=True, text=True).stdout.strip(),
    "grid_router_so": os.path.relpath(so, export),
    "grid_router_so_sha256": sha(so),
    "krt_path": krt_path,
    "export_patch": read(os.path.join(logs, "export-patch.txt")) or None,
    "smoke_board": "kicad_files/cap_chain.kicad_pcb",
    "smoke_exit": int(pure_exit),
    "smoke_output": os.path.exists(pure_out),
    "smoke_output_sha256": sha(pure_out) if os.path.exists(pure_out) else None,
    "smoke_fallbacks": grep(os.path.join(smoke, "pure", "route.log"),
                            "kicad-cli", "pcbnew", "raster", "fallback", "oracle"),
    "shim_board": shim_board,
    "shim_exit": int(shim_exit),
    "shim_output": os.path.exists(shim_out),
    "shim_calls": shim_calls,
    "shim_oracle_succeeded": bool(shim_calls) and all("\texit=0\t" in l for l in shim_calls),
    "shim_log_lines": grep(os.path.join(smoke, "shim", "route.log"), "kicad-cli", "oracle", "exact fill"),
}
json.dump(env, open(out, "w"), indent=2)
print(json.dumps({k: env[k] for k in ("repo_unchanged", "smoke_exit", "smoke_output",
      "grid_router_so_sha256", "rustc", "shim_exit", "shim_oracle_succeeded")}, indent=1))
print(f"shim calls: {len(shim_calls)}")
sys.exit(0 if env["repo_unchanged"] and env["smoke_exit"] == 0 and env["smoke_output"] else 1)
PY
