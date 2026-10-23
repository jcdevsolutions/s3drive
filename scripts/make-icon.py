#!/usr/bin/env python3
"""Generate the S3Dive source icon (1024x1024 PNG).

Reproducible: `python3 scripts/make-icon.py assets/app-icon.png`, then
`pnpm tauri icon assets/app-icon.png` to regenerate every platform bundle icon.
"""

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

SIZE = 1024
RADIUS = 232


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def gradient(size: int, top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    img = Image.new("RGB", (size, size))
    draw = ImageDraw.Draw(img)
    for y in range(size):
        t = y / (size - 1)
        draw.line(
            [(0, y), (size, y)],
            fill=(
                int(lerp(top[0], bottom[0], t)),
                int(lerp(top[1], bottom[1], t)),
                int(lerp(top[2], bottom[2], t)),
            ),
        )
    return img


def rounded_mask(size: int, radius: int, inset: int = 0) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle(
        [inset, inset, size - 1 - inset, size - 1 - inset],
        radius=radius,
        fill=255,
    )
    return mask


def main() -> int:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "assets/app-icon.png")
    out.parent.mkdir(parents=True, exist_ok=True)

    base = gradient(SIZE, (14, 22, 38), (7, 11, 20)).convert("RGBA")
    base.putalpha(rounded_mask(SIZE, RADIUS))

    # Soft inner glow near the top so the tile does not read as flat black.
    glow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([130, -320, SIZE - 130, 520], fill=(45, 212, 191, 58))
    base.alpha_composite(glow.filter(ImageFilter.GaussianBlur(90)))

    art = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(art)

    # Water line: three stacked bars fading downwards, the "bucket" level markers.
    bar_colors = [(94, 234, 212, 235), (45, 212, 191, 175), (13, 148, 136, 120)]
    for index, color in enumerate(bar_colors):
        y = 640 + index * 92
        width = 470 - index * 92
        draw.rounded_rectangle(
            [SIZE / 2 - width / 2, y, SIZE / 2 + width / 2, y + 40],
            radius=20,
            fill=color,
        )

    # Dive mark: a thick chevron pointing into the depths.
    draw.polygon(
        [
            (SIZE / 2 - 250, 220),
            (SIZE / 2, 470),
            (SIZE / 2 + 250, 220),
            (SIZE / 2 + 250, 348),
            (SIZE / 2, 600),
            (SIZE / 2 - 250, 348),
        ],
        fill=(240, 253, 250, 245),
    )

    # Accent stroke around the tile.
    draw.rounded_rectangle(
        [16, 16, SIZE - 17, SIZE - 17],
        radius=RADIUS - 8,
        outline=(45, 212, 191, 64),
        width=6,
    )

    art.putalpha(Image.composite(art.split()[3], Image.new("L", (SIZE, SIZE), 0), rounded_mask(SIZE, RADIUS)))
    base.alpha_composite(art)

    base.save(out)
    print(f"wrote {out} ({base.size[0]}x{base.size[1]})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
