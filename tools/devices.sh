# The devices the example tours run on — **the one table** tools/tours.sh and
# tools/readme-gifs.sh both read, so the checks and the README's pictures are of the same
# panels. One line a tour: its name, the example, the virtual screen Xvfb has to hold the
# window, and the example's own arguments (its size in px and, where the example is sized by
# the panel, the panel in mm). The palette sheet carries its size itself (`palette_sheet::SIZE`).
#
# The kiosk's three are the devices its heading lists; the demo and the custom chrome are the
# reference 1024 x 600 at the density-unaware fallback, as a developer's first run is.
DEVICES="
demo     demo           1280x800   --size=1024x600
gesture  demo           1280x800   --size=1024x600 --nav=gesture
console  console        1280x800   --size=1280x800
kiosk    kiosk          1280x2700  --size=1080x2560 --panel-mm=380x900
counter  kiosk          1400x1000  --size=1280x800 --panel-mm=217x136
compact  kiosk          1400x1000  --size=480x800 --panel-mm=56x94
chrome   custom_chrome  1280x800   --size=1024x600
palette  palette_sheet  1500x5900  --panel-mm=305x381
"

# The tour names, in the table's order.
device_names() { echo "$DEVICES" | awk 'NF {print $1}'; }
# The example a tour runs.
device_example() { echo "$DEVICES" | awk -v n="$1" '$1==n {print $2}'; }
# The virtual screen a tour needs.
device_screen() { echo "$DEVICES" | awk -v n="$1" '$1==n {print $3}'; }
# The example's arguments, as one line.
device_args() { echo "$DEVICES" | awk -v n="$1" '$1==n {$1=$2=$3=""; sub(/^ +/, ""); print}'; }
# Every example the table names, once each, as `--example` flags for cargo build.
device_build_flags() { echo "$DEVICES" | awk 'NF {print "--example " $2}' | sort -u | tr '\n' ' '; }
