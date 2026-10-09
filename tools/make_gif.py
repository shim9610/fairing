#!/usr/bin/env python3
"""Stitch the frames an example tour records with `--record` into a looping GIF.

    python3 tools/make_gif.py <frames dir> <out.gif> <width> [options]

`--record` writes `0000.png`, `0001.png`, ... 30 a second into one folder per animated stretch
(guide 01 §5). This turns one such folder into a GIF: every frame resized to `width`, one palette
for the whole clip, built from frames sampled across it so that colours do not flicker from frame
to frame, and Pillow's own frame deltas, which merge identical frames and store only the part of
each frame that changed. The deltas are what keep the still stretches nearly free.

Dithering is off by default: on flat UI it only adds noise that every frame has to carry. Turn it
on (`--dither`) for a clip full of photographs if the banding shows, at about 1.6 times the size.

Needs Pillow 9.1 or newer (`pip install pillow`). `tools/readme-gifs.sh` runs the tours and calls
this for the animations in the README.
"""

import argparse
import os
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("make_gif.py needs Pillow: pip install pillow")



def recorded_fps(frames: Path) -> float:
    """The rate the frames were kept at, as the recorder wrote it beside them (`fps`)."""
    try:
        return float((frames / "fps").read_text().strip())
    except (OSError, ValueError):
        sys.exit(f"no fps file in {frames} - the frames were not written by a tour's --record")


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("frames", type=Path, help="a folder of 0000.png, 0001.png, ...")
    p.add_argument("out", type=Path, help="the GIF to write")
    p.add_argument("width", type=int, help="the GIF's width in pixels; the height keeps the ratio")
    p.add_argument("--step", type=int, default=1, help="keep every Nth frame (1 = 30 fps, 2 = 15)")
    p.add_argument("--colors", type=int, default=255, help="palette size (at most 256)")
    p.add_argument("--dither", action="store_true", help="Floyd-Steinberg dithering")
    p.add_argument("--crop", help="x0,y0,x1,y1 in the recorded frame, before resizing")
    p.add_argument("--hold-last", type=float, default=1.2, help="seconds on the last frame")
    p.add_argument("--trim-start", type=int, default=0, help="recorded frames to drop at the start")
    p.add_argument("--trim-end", type=int, default=0, help="recorded frames to drop at the end")
    return p.parse_args()


def main() -> None:
    a = parse_args()
    files = sorted(a.frames.glob("*.png"))
    if a.trim_end:
        files = files[: -a.trim_end]
    files = files[a.trim_start :: max(a.step, 1)]
    if not files:
        sys.exit(f"no frames in {a.frames} - run the tour with --tour <dir> --record first")
    crop = tuple(int(v) for v in a.crop.split(",")) if a.crop else None

    frames = []
    for f in files:
        im = Image.open(f).convert("RGB")
        if crop:
            im = im.crop(crop)
        height = round(im.height * a.width / im.width)
        frames.append(im.resize((a.width, height), Image.Resampling.LANCZOS))

    # One palette for the clip, from up to 16 frames spread across it, stacked into one image.
    count = min(len(frames), 16)
    picks = sorted({round(i * (len(frames) - 1) / max(count - 1, 1)) for i in range(count)})
    w, h = frames[0].size
    mosaic = Image.new("RGB", (w, h * len(picks)))
    for k, i in enumerate(picks):
        mosaic.paste(frames[i], (0, h * k))
    palette = mosaic.quantize(colors=min(a.colors, 256), method=Image.Quantize.MEDIANCUT)

    dither = Image.Dither.FLOYDSTEINBERG if a.dither else Image.Dither.NONE
    quantised = [f.quantize(palette=palette, dither=dither) for f in frames]

    frame_ms = round(1000 * max(a.step, 1) / recorded_fps(a.frames))
    durations = [frame_ms] * len(quantised)
    durations[-1] = max(frame_ms, round(a.hold_last * 1000))
    a.out.parent.mkdir(parents=True, exist_ok=True)
    quantised[0].save(
        a.out,
        save_all=True,
        append_images=quantised[1:],
        duration=durations,
        loop=0,
        # Keep each frame in place for the next: Pillow stores only what changed.
        disposal=1,
        optimize=False,
    )
    size = os.path.getsize(a.out)
    seconds = sum(durations) / 1000
    print(f"{a.out}: {len(quantised)} frames, {w}x{h}, {seconds:.1f} s, {size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
