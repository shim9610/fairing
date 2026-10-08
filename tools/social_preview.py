#!/usr/bin/env python3
"""Build the repository's social preview card, docs/images/social-preview.png (1280 x 640).

    python3 tools/social_preview.py

The card is the brand splash with the name, the one-line description and a row of tags over the
open water above the manta. It needs Pillow (`pip install pillow`) and reads only files in this
repository: the splash from assets/brand and the faces from assets/fonts. GitHub takes the card
from the repository settings (Settings, General, Social preview), not from this file.
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
SPLASH = ROOT / "assets/brand/abyss-splash-1920x1080.webp"
BOLD = ROOT / "assets/fonts/NotoSansKR-Bold.ttf"
REGULAR = ROOT / "assets/fonts/NotoSansKR-Regular.ttf"
OUT = ROOT / "docs/images/social-preview.png"

SIZE = (1280, 640)
TAGS = ["Rust", "egui", "touch", "kiosk", "MIT"]


def main() -> None:
    # 2:1 out of the 16:9 splash, keeping the manta and the light from the surface.
    card = Image.open(SPLASH).convert("RGB").crop((0, 90, 1920, 1050)).resize(SIZE, Image.LANCZOS)

    # A soft darkening behind the text, so it reads over the light rays.
    veil = Image.new("L", SIZE, 0)
    ImageDraw.Draw(veil).rounded_rectangle((20, 40, 760, 330), radius=60, fill=120)
    veil = veil.filter(ImageFilter.GaussianBlur(45))
    card = Image.composite(Image.new("RGB", SIZE, (3, 12, 36)), card, veil)

    draw = ImageDraw.Draw(card)
    title = ImageFont.truetype(str(BOLD), 112)
    line = ImageFont.truetype(str(REGULAR), 32)
    tag = ImageFont.truetype(str(REGULAR), 22)

    x = 78
    draw.text((x, 30), "fairing", font=title, fill=(242, 249, 255))
    draw.text((x + 4, 180), "A touchscreen shell for embedded devices,", font=line, fill=(208, 227, 246))
    draw.text((x + 4, 222), "built on egui", font=line, fill=(208, 227, 246))

    left, top = x + 4, 284
    for text in TAGS:
        width = draw.textlength(text, font=tag)
        draw.rounded_rectangle((left, top, left + width + 24, top + 36), radius=18, outline=(150, 195, 240), width=2)
        draw.text((left + 12, top + 3), text, font=tag, fill=(218, 234, 250))
        left += width + 34

    card.save(OUT, optimize=True)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size // 1024} KiB)")


if __name__ == "__main__":
    main()
