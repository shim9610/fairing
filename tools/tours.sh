#!/usr/bin/env bash
# Run the example tours as **checks**, stills only, and fail if any step of any script failed.
#
#   tools/tours.sh            # every tour, into target/tours/<name>/
#   tools/tours.sh console    # one of: demo console kiosk counter compact chrome palette
#
# A tour presses what its script names (`Act::Tap(Spot::Text(..))`, `Act::Pin`, …) and says what it expects to
# see (`Act::Expect`) before each picture. A step that cannot find its target, or an expectation
# that does not hold, is logged as an error and the example exits non-zero at the end — so a
# change that moves a row, renames a label or breaks a flow fails here rather than leaving a wrong
# picture under the right file name. tools/readme-gifs.sh runs the same scripts with `--record`
# to rebuild the README's images; this one is the quick pass CI runs.
#
# Needs xvfb-run (Debian/Ubuntu: xvfb) and Mesa. The stills take about three minutes on a software
# rasteriser. Each tour's full log is left at target/tours/<name>.log.
set -uo pipefail
cd "$(dirname "$0")/.."
command -v xvfb-run >/dev/null || { echo "tours.sh needs xvfb-run (the xvfb package)" >&2; exit 1; }

out=${TOURS_OUT:-target/tours}
only=${1:-}
cargo build -p fairing --features runner-x11 --example demo --example console --example kiosk \
    --example custom_chrome --example palette_sheet || exit 1
mkdir -p "$out"

failed=0
# tour <name> <example> <virtual screen> <example args...>
tour() {
    local name=$1 example=$2 screen=$3
    shift 3
    [ -n "$only" ] && [ "$only" != "$name" ] && return 0
    rm -rf "${out:?}/$name"
    local log="$out/$name.log"
    if LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a -s "-screen 0 ${screen}x24" \
        "target/debug/examples/$example" --tour "$out/$name" "$@" >"$log" 2>&1; then
        echo "ok    $name - $(grep -o 'tour finished.*' "$log" | tail -n1)"
    else
        failed=1
        echo "FAIL  $name"
        # winit's BadWindow on the way out under Xvfb is noise, not a step that failed.
        grep -E '\[ERROR\]' "$log" | grep -v 'X11 error' | sed 's/^/      /'
    fi
}

tour demo    demo    1280x800  --size=1024x600
tour console console 1280x800  --size=1280x800
tour kiosk   kiosk   1280x2700 --size=1080x2560 --panel-mm=380x900
tour counter kiosk   1400x1000 --size=1280x800  --panel-mm=345x215 --layout=counter
tour compact kiosk   1400x1000 --size=480x320   --panel-mm=108x65  --layout=compact
tour chrome  custom_chrome 1280x800 --size=1024x600
tour palette palette_sheet 1500x5900 --size=1440x5800 --panel-mm=305x381

if [ "$failed" -ne 0 ]; then
    echo "some tours failed - the logs are under $out/" >&2
    exit 1
fi
echo "every tour passed"
