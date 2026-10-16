#!/usr/bin/env bash
# Opens a board in the release GUI under Xvfb, optionally sends keys, takes the
# app's own screenshot and compares it with the golden PNG.
#
#   tools/gui/smoke.sh <fixture.dsn> [--keys "<xdotool keys>"]
#
# Environment:
#   FREEROUTING_GUI_BIN  the binary (default target/release/freerouting-gui)
#   GOLDEN               the golden PNG (default tools/gui/golden/<fixture stem>.png)
#   UPDATE_GOLDEN=1      write the screenshot as the golden instead of comparing
#   SMOKE_OUT            where to keep the screenshot (default: a temp directory)
#
# Exit 0 when at most 1 % of the pixels differ (compare -metric AE -fuzz 10%),
# 1 when more do (the count is printed), 2 on a setup problem.
#
# The window is waited for with `xdotool search --sync`, the board with the
# app's `--control-dir` marker file; nothing here sleeps for a fixed time.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

if [ "${WGPU_BACKEND:-vulkan}" != vulkan ]; then
    echo "smoke.sh: WGPU_BACKEND=${WGPU_BACKEND} is not supported; the gl backend panics with" >&2
    echo "'Invalid surface' under Xvfb. Use vulkan (mesa-vulkan-drivers, lavapipe)." >&2
    exit 2
fi
export WGPU_BACKEND=vulkan

fixture=${1:-}
[ -n "$fixture" ] || { echo "usage: smoke.sh <fixture.dsn> [--keys \"<xdotool keys>\"]" >&2; exit 2; }
shift
keys=
while [ $# -gt 0 ]; do
    case $1 in
        --keys) keys=${2:-}; shift 2 ;;
        *) echo "smoke.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -f "$fixture" ] || { echo "smoke.sh: no such file: $fixture" >&2; exit 2; }
stem=$(basename "$fixture"); stem=${stem%.*}
golden=${GOLDEN:-$here/golden/$stem.png}
export SMOKE_FIXTURE=$(cd "$(dirname "$fixture")" && pwd)/$(basename "$fixture")
export SMOKE_KEYS=$keys
export SMOKE_BIN=${FREEROUTING_GUI_BIN:-$root/target/release/freerouting-gui}
[ -x "$SMOKE_BIN" ] || { echo "smoke.sh: build the release binary first (cargo build --release -p freerouting-gui)" >&2; exit 2; }
for tool in xvfb-run xdotool compare; do
    command -v "$tool" > /dev/null || { echo "smoke.sh: $tool is not installed" >&2; exit 2; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
shot=${SMOKE_OUT:-$work/shot.png}
export SMOKE_SHOT=$shot SMOKE_WORK=$work

# Runs inside Xvfb.
cat > "$work/inner.sh" <<'INNER'
#!/usr/bin/env bash
set -u
[ -n "${SMOKE_DEBUG:-}" ] && set -x
ctl=$SMOKE_WORK/ctl
mkdir -p "$ctl"
"$SMOKE_BIN" "$SMOKE_FIXTURE" --screenshot "$SMOKE_SHOT" --control-dir "$ctl" --exit &
pid=$!
# wait_for <seconds> <test command...>: polls without a fixed sleep.
wait_for() {
    local deadline=$((SECONDS + $1)); shift
    until "$@"; do
        [ $SECONDS -lt $deadline ] || return 1
        timeout 0.1 tail -f /dev/null
    done
    return 0
}
wid=$(timeout 60 xdotool search --sync --onlyvisible --name . | head -1)
[ -n "$wid" ] || { echo "smoke.sh: the window did not appear" >&2; kill $pid; exit 2; }
wait_for 120 test -e "$ctl/ready" || { echo "smoke.sh: the board did not load" >&2; kill $pid; exit 2; }
# There is no window manager to focus the window, and winit drops keys while
# it has no focus. Park the pointer on the status bar so no hover text or
# highlight changes the picture.
xdotool windowfocus --sync "$wid"
xdotool mousemove --window "$wid" 640 790
if [ -n "$SMOKE_KEYS" ]; then
    # shellcheck disable=SC2086
    xdotool key --clearmodifiers $SMOKE_KEYS
fi
touch "$ctl/shoot"
wait_for 60 test -s "$SMOKE_SHOT" || { echo "smoke.sh: no screenshot was written" >&2; kill $pid; exit 2; }
wait $pid
INNER
chmod +x "$work/inner.sh"
xvfb-run -a -s '-screen 0 1280x800x24' bash "$work/inner.sh" 2> "$work/log" || {
    grep -v -e MESA -e libEGL "$work/log" >&2
    exit 2
}

if [ "${UPDATE_GOLDEN:-0}" = 1 ]; then
    mkdir -p "$(dirname "$golden")"
    cp "$shot" "$golden"
    echo "smoke.sh: wrote $golden"
    exit 0
fi
[ -f "$golden" ] || { echo "smoke.sh: no golden $golden (UPDATE_GOLDEN=1 writes one)" >&2; exit 2; }

# compare prints the number of differing pixels on stderr.
ae=$(compare -metric AE -fuzz 10% "$shot" "$golden" null: 2>&1 | grep -o '^[0-9.e+]*' | head -1)
ae=${ae%.*}
read -r w h <<< "$(identify -format '%w %h' "$golden")"
size=$((w * h))
limit=$((size / 100))
echo "smoke.sh: $stem: AE=$ae of $size pixels (limit $limit)"
if [ "${ae:-0}" -le "$limit" ]; then
    exit 0
fi
echo "smoke.sh: FAIL: more than 1 % of the pixels differ from $golden" >&2
[ -n "${SMOKE_OUT:-}" ] || echo "smoke.sh: set SMOKE_OUT=<file> to keep the screenshot" >&2
exit 1
