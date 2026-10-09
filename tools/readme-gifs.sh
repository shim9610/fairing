#!/usr/bin/env bash
# Rebuild the README's animations and stills (docs/images/*.gif, *.png) from the example tours.
#
#   tools/readme-gifs.sh
#
# Each tour runs with `--record` (a fixed 60 Hz clock, so a software rasteriser records as smoothly
# as a GPU; guide 01 §5) and the stretch it records is stitched by tools/make_gif.py. The tours run
# under xvfb-run with Mesa's software rasteriser, as the committed GIFs were made: the kiosk wants a
# 1080 x 2560 window, which no desktop screen holds, and its taps are written for that size.
#
# Every tour is also a check (tools/tours.sh): a step that cannot find what it names, or an
# expectation that does not hold, exits the example non-zero and stops this script before an
# image is rebuilt from a flow that went wrong.
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

# The stills: the demo's and the console's in a 256-colour palette, the kiosk's photos in full colour
# at 480 wide. social-preview.png is tools/social_preview.py's.
python3 - "$out" <<'PY'
import sys
from PIL import Image

out = sys.argv[1]
def still(shot, name, width=None, bottom=None, palette=True):
    im = Image.open(f"{out}/{shot}").convert("RGB")
    if bottom:
        im = im.crop((0, im.height - bottom, im.width, im.height))
    if width:
        im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    if palette:
        im = im.quantize(256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    im.save(f"docs/images/{name}", optimize=True)
    print(f"docs/images/{name}: {im.width}x{im.height}")

still("demo/10-shade-open.png", "shade-curtain.png")
still("demo/16-settings-wifi.png", "settings.png")
still("demo/11a-osk-compose.png", "hangul-keyboard.png", bottom=452)
still("demo/32-unlock-prompt.png", "unlock-pin.png")
still("demo/32e-pattern-drawn.png", "unlock-pattern.png")
still("console/01-overview.png", "console.png")
still("kiosk/04-menu-filled.png", "kiosk-menu.png", width=480, palette=False)
still("kiosk/05-cart.png", "kiosk-cart.png", width=480, palette=False)
PY
