#!/usr/bin/env bash
# R01.3/R01.4: the Linux AppImage, built with appimagetool from an AppDir assembled here.
#   make_appimage.sh <dir with freerouting-rs and freerouting-gui> <out dir>
#
# Why not `cargo packager -f appimage`: with cargo-packager 0.11.8 (linuxdeploy inside) the
# AppImage built and started on 2026-10-17 on Ubuntu 24.04 with Rust stable, but the GUI binary
# inside it was rewritten to 18 MB (the input is 32 MB) and hung before drawing its first frame
# (no screenshot, no error), while the same input binary worked, also after a plain strip or
# patchelf. This script touches no binary, and its AppRun passes the working directory and the
# arguments through, so relative board paths work. No system library is bundled.
#
# APPIMAGETOOL=<path> uses a local appimagetool instead of downloading the pinned one.
set -euo pipefail

bins="$(cd "$1" && pwd)"
out="$2"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
version="$(python3 "$repo/tools/release/check_version.py" | awk '$1 == "workspace" {print $2}')"
arch=x86_64
# appimagetool "continuous" build 313 of 2026-10-04; the hash makes a changed download fail loudly.
tool_url="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage"
tool_sha256="95cbe7cce9717fce90c484e34052ee7c7f1d7635b33c12525b4776826a7d29b6"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tool="${APPIMAGETOOL:-}"
if [ -z "$tool" ]; then
  tool="$work/appimagetool"
  curl -sSL --fail -o "$tool" "$tool_url"
  echo "$tool_sha256  $tool" | sha256sum -c - >&2
  chmod +x "$tool"
fi

app="$work/freerouting-rs.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/doc/freerouting-rs"
cp "$bins/freerouting-gui" "$bins/freerouting-rs" "$app/usr/bin/"
cp "$repo/LICENSE" "$repo/THIRD_PARTY_NOTICES.md" "$app/usr/share/doc/freerouting-rs/"
cp -r "$repo/LICENSES" "$app/usr/share/doc/freerouting-rs/LICENSES"
cp "$repo/crates/freerouting-gui/icons/icon-256.png" "$app/freerouting-gui.png"
cp "$app/freerouting-gui.png" "$app/.DirIcon"
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
# Starts the GUI with the caller's working directory and arguments.
HERE="$(dirname "$(readlink -f "$0")")"
export PATH="$HERE/usr/bin:$PATH"
exec "$HERE/usr/bin/freerouting-gui" "$@"
RUN
chmod +x "$app/AppRun"
cat > "$app/freerouting-gui.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=freerouting-rs
Comment=PCB autorouter and viewer, a Rust port of Freerouting
Exec=freerouting-gui %F
Icon=freerouting-gui
Terminal=false
Categories=Development;Electronics;Engineering;
DESKTOP
cp "$app/freerouting-gui.desktop" "$app/usr/share/applications/"

mkdir -p "$out"
dest="$out/freerouting-rs-${version}-linux-${arch}.AppImage"
# SOURCE_DATE_EPOCH (if set) makes the squashfs timestamps fixed.
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$dest" >&2
echo "$dest"
