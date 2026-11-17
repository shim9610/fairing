#!/usr/bin/env bash
# Rebuild the README's animations and stills (docs/images/*.gif, *.png) from the example tours.
#
#   tools/readme-gifs.sh
#
# Each tour runs with `--record` and a fixed 60 Hz input clock (guide 01 §5).
# tools/make_gif.py stitches the recorded stretch. The tours run
# under xvfb-run with Mesa's software rasteriser.
#
# The tours check named targets, explicit assertions and capture/write results.
# A non-zero example exit stops this script. Settle warnings and unasserted visual
# states still need review; recording does not establish that the flow is correct.
#
# Needs xvfb-run (Debian/Ubuntu: xvfb), Mesa and Pillow (pip install pillow). The frames take about
# 2 GB under target/readme-gifs and stay there until the next run.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v xvfb-run >/dev/null || { echo "readme-gifs.sh needs xvfb-run (the xvfb package)" >&2; exit 1; }
# shellcheck source=tools/devices.sh
. tools/devices.sh

out=target/readme-gifs
cargo build -p fairing --features runner-x11 --example demo --example console

# tour <name> — the device tools/devices.sh lists under that name, recorded.
tour() {
    local name=$1 example screen
    example=$(device_example "$name")
    screen=$(device_screen "$name")
    rm -rf "${out:?}/$example"
    echo "recording $name ($example)"
    # shellcheck disable=SC2046 # the example's arguments are meant to split
    LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a -s "-screen 0 ${screen}x24" \
        "target/debug/examples/$example" --tour "$out/$example" --record $(device_args "$name")
}

tour demo
tour console

python3 tools/make_gif.py "$out/demo/demo-desktop" docs/images/demo.gif 640
python3 tools/make_gif.py "$out/console/console-shade" docs/images/console-shade.gif 640

# The stills use a 256-colour palette. social-preview.png is tools/social_preview.py's.
python3 - "$out" <<'PY'
import sys
from PIL import Image

out = sys.argv[1]
def still(shot, name, width=None, palette=True):
    im = Image.open(f"{out}/{shot}").convert("RGB")
    if width:
        im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    if palette:
        im = im.quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    im.save(f"docs/images/{name}", optimize=True)
    print(f"docs/images/{name}: {im.width}x{im.height}")

still("demo/10-shade-open.png", "shade-curtain.png")
still("demo/16-settings-wifi.png", "settings.png")
still("demo/11a-osk-keyboard.png", "hangul-keyboard.png")
still("console/01-overview.png", "console.png")
PY
