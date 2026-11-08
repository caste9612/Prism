"""Generates the icons of Prism from the approved mark (ADR-0019, REVIEW D52).

The mark is a prism made of layers on a black rounded tile: the white tip, then orange, blue and
green. It is drawn here from its geometry (a 64-unit square, the same figure as
`ui/src/components/Logo.svelte`), at four times the target size and scaled down, so every size
is sharp. Writes the files listed in `crates/prism-app/tauri.conf.json` (`bundle.icon`) plus the
1024 px source.

Usage (needs Pillow): python scripts/make-icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parents[1] / "crates" / "prism-app" / "icons"

TILE = "#111418"
TILE_RADIUS = 14
LAYERS = [
    ("#ffffff", [(32, 12), (39.37, 26), (24.63, 26)]),
    ("#f04e23", [(23.58, 28), (40.42, 28), (44.11, 35), (19.89, 35)]),
    ("#5b9dff", [(18.84, 37), (45.16, 37), (48.32, 43), (15.68, 43)]),
    ("#3fbf6f", [(14.63, 45), (49.37, 45), (52, 50), (12, 50)]),
]
SUPERSAMPLE = 4


def render(size: int) -> Image.Image:
    big = size * SUPERSAMPLE
    k = big / 64
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle((0, 0, big - 1, big - 1), radius=TILE_RADIUS * k, fill=TILE)
    for color, points in LAYERS:
        draw.polygon([(x * k, y * k) for x, y in points], fill=color)
    return img.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in [
        ("source.png", 1024),
        ("icon.png", 512),
        ("128x128@2x.png", 256),
        ("128x128.png", 128),
        ("32x32.png", 32),
    ]:
        render(size).save(OUT / name)
    sizes = [16, 24, 32, 48, 64, 256]
    images = [render(s) for s in sizes]
    images[-1].save(OUT / "icon.ico", format="ICO", sizes=[(s, s) for s in sizes], append_images=images[:-1])
    print(f"icons written to {OUT}")


if __name__ == "__main__":
    main()
