#!/usr/bin/env bash
# Rebuild the README's animations (docs/images/*.gif) from the example tours.
#
#   tools/readme-gifs.sh
#
# Each tour runs with `--record` (a fixed 60 Hz clock, so a software rasteriser records as smoothly
# as a GPU; guide 01 §5) and the stretch it records is stitched by tools/make_gif.py. The tours run
# under xvfb-run with Mesa's software rasteriser, as the committed GIFs were made: the kiosk wants a
# 1080 x 2560 window, which no desktop screen holds, and its taps are written for that size.
#
# Needs xvfb-run (Debian/Ubuntu: xvfb), Mesa and Pillow (pip install pillow). The frames take about
# 2 GB under target/readme-gifs and stay there until the next run.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v xvfb-run >/dev/null || { echo "readme-gifs.sh needs xvfb-run (the xvfb package)" >&2; exit 1; }

out=target/readme-gifs
cargo build -p fairing --features runner-x11 --example demo --example console --example kiosk

# tour <example> <virtual screen> <example args...>
tour() {
    local example=$1 screen=$2
    shift 2
    rm -rf "${out:?}/$example"
    echo "recording $example"
    LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a -s "-screen 0 ${screen}x24" \
        "target/debug/examples/$example" --tour "$out/$example" --record "$@"
}

tour demo 1280x800 --size=1024x600
tour console 1280x800 --size=1280x800
tour kiosk 1280x2700 --size=1080x2560 --panel-mm=380x900

python3 tools/make_gif.py "$out/demo/demo-desktop" docs/images/demo.gif 640
python3 tools/make_gif.py "$out/console/console-shade" docs/images/console-shade.gif 640
python3 tools/make_gif.py "$out/kiosk/kiosk-language" docs/images/kiosk-language.gif 360
