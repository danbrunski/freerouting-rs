#!/usr/bin/env bash
# R01.6 fallback when `cargo packager --formats app,dmg` fails: build freerouting-rs.app by hand
# and wrap it in a compressed DMG with hdiutil. macOS only.
#   macos_dmg.sh <dir with the universal freerouting and freerouting-gui> <out dir>
set -euo pipefail
bins="$1"
out="$2"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
version="$(python3 "$repo/tools/release/check_version.py" | awk '$1 == "workspace" {print $2}')"
app="$out/freerouting-rs.app"
rm -rf "$app" "$out"/*.dmg
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bins/freerouting-gui" "$bins/freerouting" "$app/Contents/MacOS/"
cp "$repo/crates/freerouting-gui/icons/icon.icns" "$app/Contents/Resources/icon.icns"
cp "$repo/LICENSE" "$repo/THIRD_PARTY_NOTICES.md" "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>freerouting-rs</string>
  <key>CFBundleDisplayName</key><string>freerouting-rs</string>
  <key>CFBundleIdentifier</key><string>com.github.danbrunski.freerouting-rs</string>
  <key>CFBundleExecutable</key><string>freerouting-gui</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
hdiutil create -volname "freerouting-rs" -srcfolder "$app" -ov -format UDZO "$out/freerouting-rs_${version}_universal.dmg"
ls -l "$out"
