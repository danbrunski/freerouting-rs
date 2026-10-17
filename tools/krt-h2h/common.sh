# Shared settings of the X01 head-to-head scripts (sourced, not run).
#
# X01_DIR   work directory outside both source repositories
#           (default ~/.cache/freerouting-rs/x01)
# KRT_REPO  the KiCadRoutingTools checkout, read only through git (default ../KiCadRoutingTools)
# KRT_PIN   the KRT commit X01 is registered at
# JAVA_PIN  the Java fork commit of the java arm (the J01 pin)

H2H_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$H2H_DIR/../.." && pwd)
X01_DIR=${X01_DIR:-$HOME/.cache/freerouting-rs/x01}
KRT_REPO=${KRT_REPO:-$REPO_ROOT/../KiCadRoutingTools}
KRT_PIN=${KRT_PIN:-0486e799144050d4e826f71bb31acdc7275af2a2}
JAVA_PIN=${JAVA_PIN:-a773a70ec}
ORACLE_DIR="$REPO_ROOT/tools/kicad-oracle"
KRT_EXPORT="$X01_DIR/krt"
KRT_VENV="$X01_DIR/venv"
# A directory of links to /usr/bin with every kicad* name left out, so the krt arm finds no
# kicad-cli (KRT also probes /usr/lib/kicad/bin/kicad-cli, absent on this install; the check
# in no_kicad_path refuses if it appears).
NOKICAD_BIN="$X01_DIR/nokicad-bin"
SHIM_BIN="$X01_DIR/shim-bin"

# Builds $NOKICAD_BIN once and prints the PATH of the krt arm.
no_kicad_path() {
  if [[ ! -d "$NOKICAD_BIN" ]]; then
    mkdir -p "$NOKICAD_BIN"
    local f name
    for f in /usr/bin/*; do
      name=$(basename "$f")
      [[ "$name" == kicad* || "$name" == pcbnew* || "$name" == eeschema* ]] && continue
      ln -s "$f" "$NOKICAD_BIN/$name"
    done
  fi
  if [[ -e /usr/lib/kicad/bin/kicad-cli || -e /Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli ]]; then
    echo "a kicad-cli at one of KRT's fixed candidate paths would be reachable" >&2
    return 1
  fi
  echo "$KRT_VENV/bin:$NOKICAD_BIN"
}

# Prints the PATH of the krt-kicad arm: the krt PATH plus the shim as kicad-cli.
shim_path() {
  mkdir -p "$SHIM_BIN"
  ln -sf "$H2H_DIR/kicad-cli-shim.sh" "$SHIM_BIN/kicad-cli"
  local base
  base=$(no_kicad_path) || return 1
  echo "$SHIM_BIN:$base"
}

load1() { cut -d' ' -f1 /proc/loadavg; }

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
