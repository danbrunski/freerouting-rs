#!/usr/bin/env bash
# End-to-end check of the GUI's front-end mode (G08.6): the release GUI runs
# `-de <fixture> -do <tmp>/out.ses -dct 0` under Xvfb, routes, writes the
# session and exits; the CLI then imports that session with `-drc` and must
# report no import errors.
#
#   tools/gui/frontend-smoke.sh <fixture.dsn> [max passes]
#
# Environment:
#   FREEROUTING_GUI_BIN  the GUI (default target/release/freerouting-gui)
#   FREEROUTING_CLI_BIN  the CLI (default target/release/freerouting-rs)
#   SMOKE_OUT            keep the last screenshot taken while the job ran here
#   SMOKE_TIMEOUT        seconds to wait for the GUI to exit (default 120)
#
# Exit 0 when the GUI exits 0 with a session, and the CLI imports it with
# "SES file loaded for DRC" and no "(N errors)". 1 when a check fails, 2 on a
# setup problem. Nothing here sleeps for a fixed time: screenshots are taken in
# a loop until the GUI exits, and the exit code is read with `wait`.
set -u

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

if [ "${WGPU_BACKEND:-vulkan}" != vulkan ]; then
    echo "frontend-smoke.sh: WGPU_BACKEND=${WGPU_BACKEND} is not supported; use vulkan (lavapipe)." >&2
    exit 2
fi
export WGPU_BACKEND=vulkan

fixture=${1:-}
[ -n "$fixture" ] || { echo "usage: frontend-smoke.sh <fixture.dsn> [max passes]" >&2; exit 2; }
[ -f "$fixture" ] || { echo "frontend-smoke.sh: no such file: $fixture" >&2; exit 2; }
passes=${2:-}
export SMOKE_FIXTURE=$(cd "$(dirname "$fixture")" && pwd)/$(basename "$fixture")
export SMOKE_PASSES=$passes
export SMOKE_BIN=${FREEROUTING_GUI_BIN:-$root/target/release/freerouting-gui}
cli=${FREEROUTING_CLI_BIN:-$root/target/release/freerouting-rs}
export SMOKE_TIMEOUT=${SMOKE_TIMEOUT:-120}
for bin in "$SMOKE_BIN" "$cli"; do
    [ -x "$bin" ] || { echo "frontend-smoke.sh: build the release binaries first (cargo build --release -p freerouting-gui -p freerouting-cli): $bin" >&2; exit 2; }
done
for tool in xvfb-run import; do
    command -v "$tool" > /dev/null || { echo "frontend-smoke.sh: $tool is not installed" >&2; exit 2; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export SMOKE_WORK=$work
out=$work/out.ses
shot=${SMOKE_OUT:-$work/shot.png}
export SMOKE_SHOT=$shot

# Runs inside Xvfb.
cat > "$work/inner.sh" <<'INNER'
#!/usr/bin/env bash
set -u
args=(-de "$SMOKE_FIXTURE" -do "$SMOKE_WORK/out.ses" -dct 0)
[ -n "$SMOKE_PASSES" ] && args+=(-mp "$SMOKE_PASSES")
"$SMOKE_BIN" "${args[@]}" &
pid=$!
# Take screenshots while the job runs; the last one before the exit is kept.
# No window is waited for: a small board is routed and the app gone in a
# few seconds, so a wait for the window could be too late. A screenshot is
# not a fixed delay either: `import` itself takes a moment.
deadline=$((SECONDS + SMOKE_TIMEOUT))
while kill -0 $pid 2> /dev/null; do
    if [ $SECONDS -ge $deadline ]; then
        echo "frontend-smoke.sh: the GUI did not exit within ${SMOKE_TIMEOUT} s" >&2
        kill $pid
        exit 3
    fi
    import -window root "$SMOKE_SHOT.part.png" 2> /dev/null && mv "$SMOKE_SHOT.part.png" "$SMOKE_SHOT"
done
wait $pid
echo $? > "$SMOKE_WORK/gui-exit"
INNER
chmod +x "$work/inner.sh"
xvfb-run -a -s '-screen 0 1280x800x24' bash "$work/inner.sh" 2> "$work/log"
inner=$?
grep -v -e MESA -e libEGL "$work/log" >&2
if [ $inner -ne 0 ]; then
    echo "frontend-smoke.sh: FAIL: the run under Xvfb ended with $inner" >&2
    exit $((inner == 2 ? 2 : 1))
fi

gui=$(cat "$work/gui-exit" 2> /dev/null || echo missing)
if [ "$gui" != 0 ]; then
    echo "frontend-smoke.sh: FAIL: the GUI exited with $gui, not 0" >&2
    exit 1
fi
[ -s "$out" ] || { echo "frontend-smoke.sh: FAIL: no session was written to $out" >&2; exit 1; }
echo "frontend-smoke.sh: the GUI exited 0 and wrote $(wc -c < "$out") bytes"

# The CLI imports the session on top of the design (drc/runner.rs prints the
# import line, with " (N errors)" only when there were errors).
log=$work/cli.log
"$cli" -de "$SMOKE_FIXTURE" "$out" -drc > "$log" 2>&1
line=$(grep "SES file loaded for DRC" "$log" | head -1)
if [ -z "$line" ]; then
    echo "frontend-smoke.sh: FAIL: the CLI did not import the session:" >&2
    tail -5 "$log" >&2
    exit 1
fi
echo "frontend-smoke.sh: $line"
case $line in
    *" errors)"*) echo "frontend-smoke.sh: FAIL: the session has import errors" >&2; exit 1 ;;
esac
if [ -s "$shot" ]; then
    echo "frontend-smoke.sh: last screenshot of the run: $shot"
fi
exit 0
