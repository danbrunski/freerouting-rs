#!/usr/bin/env bash
# K11.1: run the IPC probe on a KiCad nightly in Docker against the local KiCad 10.
#
#   tools/kicad-ipc/run-probe.sh WORK ROUTER BOARD.kicad_pcb...
#
# WORK      an empty folder for copies and results
# ROUTER    a freerouting binary (target/release/freerouting)
# BOARD     boards to probe; each is copied with its .kicad_pro and .kicad_dru
#
# Environment:
#   IMAGE     the nightly image (default freerouting-rs/kicad-nightly, built by
#             tools/kicad-ipc/Dockerfile from the kicad-dev-nightly PPA)
#   PROTO_GEN modules generated from the nightly's api/proto (see probe.py)
#   PY        a Python with pynng and protobuf (a kicad-python venv)
#   KICAD_PY  a Python that imports KiCad 10's pcbnew (default python3)
#
# Never touches a running KiCad: the nightly runs headless in a container, and
# KiCad 10 only loads copies through pcbnew.LoadBoard.
set -euo pipefail

work=$(realpath "$1"); router=$(realpath "$2"); shift 2
here=$(cd "$(dirname "$0")" && pwd)
image=${IMAGE:-freerouting-rs/kicad-nightly}
py=${PY:?set PY to a Python with pynng and protobuf}
gen=${PROTO_GEN:?set PROTO_GEN to the generated proto modules}
kpy=${KICAD_PY:-python3}
mkdir -p "$work/home"

# The API socket lives at /w/<board>/api.sock in the container and is dialled by a
# relative path, because a full scratch path exceeds the 108-byte socket limit.
server() { # board-dir board-file
  docker run -d --rm --name "k11-api-$$" --memory=3g --user "$(id -u):$(id -g)" \
    -e HOME="$work/home" -v "$work:$work" -v "$work:/w" "$image" \
    kicad-cli-nightly api-server --socket "/w/${1#"$work"/}/api.sock" "$1/$2" >/dev/null
  for _ in $(seq 1 60); do [ -S "$1/api.sock" ] && return; sleep 1; done
  echo "the API server did not start" >&2; exit 1
}
stop() { docker stop "k11-api-$$" >/dev/null 2>&1 || true; rm -f "$1"/api.sock "$1"/api.lock "$1"/~*.lck; }
api() { (cd "$1" && shift && "$py" "$here/probe.py" --gen "$gen" --socket ipc://api.sock "$@"); }
items() { python3 -I "$here/probe.py" items "$1" "$2"; }

for board in "$@"; do
  stem=$(basename "$board" .kicad_pcb); src=$(dirname "$(realpath "$board")")
  b="$work/$stem"; mkdir -p "$b"/{swig,ipc,lock-swig,lock-ipc,edit}
  for d in swig ipc edit; do cp "$src/$stem".kicad_{pcb,pro,dru} "$b/$d/" 2>/dev/null || cp "$src/$stem.kicad_pcb" "$b/$d/"; done
  echo "== $stem"

  echo "-- export: KiCad 10 ExportSpecctraDSN, then the nightly's RunBoardJobExportSpecctraDsn"
  "$kpy" "$here/swig_side.py" export "$b/swig/$stem.kicad_pcb" "$b/swig/$stem.dsn"
  server "$b/ipc" "$stem.kicad_pcb"
  api "$b/ipc" version
  api "$b/ipc" export "$stem.kicad_pcb" "$b/ipc/$stem.dsn"
  python3 -I "$here/probe.py" dsn-summary "$b/swig/$stem.dsn" "$b/ipc/$stem.dsn" || true

  echo "-- route KiCad 10's DSN with $router"
  "$router" -de "$b/swig/$stem.dsn" -do "$b/route.ses" -mp 3 --threads 2 >"$b/route.log" 2>&1 || true
  tail -n 2 "$b/route.log"

  echo "-- import: KiCad 10 ImportSpecctraSES, then the nightly's ImportSpecctraSession"
  "$kpy" "$here/swig_side.py" import "$b/swig/$stem.kicad_pcb" "$b/route.ses" "$b/swig/imported.kicad_pcb"
  api "$b/ipc" import "$stem.kicad_pcb" "$b/route.ses"
  api "$b/ipc" save-copy "$stem.kicad_pcb" "$b/ipc/imported.kicad_pcb"
  stop "$b/ipc"
  items "$b/swig/imported.kicad_pcb" "$b/swig/imported.json"
  items "$b/ipc/imported.kicad_pcb" "$b/ipc/imported.json"
  python3 -I "$here/../kicad-reader/frame_check.py" compare "$b/swig/imported.json" "$b/ipc/imported.json" || true

  echo "-- locked items: lock 10 tracks and a via, import the same session again"
  "$kpy" "$here/swig_side.py" lock "$b/swig/imported.kicad_pcb" "$b/lock-swig/$stem.kicad_pcb" 10
  cp "$b/lock-swig/$stem.kicad_pcb" "$b/lock-ipc/"
  cp "$b/swig/$stem.kicad_pro" "$b/lock-swig/" 2>/dev/null && cp "$b/swig/$stem.kicad_pro" "$b/lock-ipc/" || true
  items "$b/lock-swig/$stem.kicad_pcb" "$b/lock-swig/before.json"
  "$kpy" "$here/swig_side.py" import "$b/lock-swig/$stem.kicad_pcb" "$b/route.ses" "$b/lock-swig/imported.kicad_pcb"
  server "$b/lock-ipc" "$stem.kicad_pcb"
  api "$b/lock-ipc" import "$stem.kicad_pcb" "$b/route.ses"
  api "$b/lock-ipc" save-copy "$stem.kicad_pcb" "$b/lock-ipc/imported.kicad_pcb"
  stop "$b/lock-ipc"
  items "$b/lock-swig/imported.kicad_pcb" "$b/lock-swig/imported.json"
  items "$b/lock-ipc/imported.kicad_pcb" "$b/lock-ipc/imported.json"
  python3 -I "$here/../kicad-reader/frame_check.py" compare "$b/lock-swig/imported.json" "$b/lock-ipc/imported.json" || true

  echo "-- an unsaved edit reaches a snapshot (K11.6)"
  server "$b/edit" "$stem.kicad_pcb"
  api "$b/edit" modified "$stem.kicad_pcb"
  api "$b/edit" add-track "$stem.kicad_pcb" BL_F_Cu "" 0 0 1000000 0 250000
  api "$b/edit" modified "$stem.kicad_pcb"
  api "$b/edit" to-string "$stem.kicad_pcb" "$b/edit/string.kicad_pcb"
  api "$b/edit" save-copy "$stem.kicad_pcb" "$b/edit/copy.kicad_pcb"
  stop "$b/edit"
  items "$b/edit/$stem.kicad_pcb" "$b/edit/disk.json"
  items "$b/edit/string.kicad_pcb" "$b/edit/string.json"
  items "$b/edit/copy.kicad_pcb" "$b/edit/copy.json"
done
