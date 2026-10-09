#!/usr/bin/env bash
# Run the example tours as **checks**, stills only, and fail if any step of any script failed.
#
#   tools/tours.sh            # every tour, into target/tours/<name>/
#   tools/tours.sh console    # one tour, by the name tools/devices.sh gives it
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
# shellcheck source=tools/devices.sh
. tools/devices.sh

out=${TOURS_OUT:-target/tours}
only=${1:-}
# A name that is not a tour runs nothing, and nothing passing is not a pass.
if [ -n "$only" ] && [ -z "$(device_example "$only")" ]; then
    echo "tours.sh: no tour named '$only' (one of: $(device_names | tr '\n' ' '))" >&2
    exit 2
fi
# shellcheck disable=SC2046 # the flags are meant to split
cargo build -p fairing --features runner-x11 $(device_build_flags) || exit 1
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
        # The steps that failed, then the end of the log: an example that could not open a window
        # or load a library says so there, in Rust's own words, before any step ran. winit's
        # BadWindow on the way out under Xvfb is noise, not a step that failed.
        grep -E '\[ERROR\]' "$log" | grep -v 'X11 error' | sed 's/^/      /'
        echo "      --- the last lines of $log:"
        tail -n 12 "$log" | sed 's/^/      /'
    fi
}

for name in $(device_names); do
    # shellcheck disable=SC2046 # the example's arguments are meant to split
    tour "$name" "$(device_example "$name")" "$(device_screen "$name")" $(device_args "$name")
done

if [ "$failed" -ne 0 ]; then
    echo "some tours failed - the logs are under $out/" >&2
    exit 1
fi
echo "every tour passed"
