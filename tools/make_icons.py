#!/usr/bin/env python3
"""Generate the application and tray icons.

Kept in the repo so the icons can be regenerated rather than treated as opaque
binaries. Run from the repository root:

    python tools/make_icons.py

Design notes:

* The glyph is three screens on a stand line, matching what the application
  actually does. It stays legible down to 16 px because the screens are simple
  filled rectangles with generous gaps rather than outlines.
* The application icon is white on an accent-coloured rounded square, which is
  the convention for Windows app icons.
* The tray icon is accent-coloured on transparency. A white glyph would vanish
  on a light taskbar and a dark one would vanish on a dark taskbar; a saturated
  mid-tone reads against both.
"""

from __future__ import annotations

import pathlib

from PIL import Image, ImageDraw

ACCENT = (59, 130, 246, 255)  # #3B82F6
WHITE = (255, 255, 255, 255)
TRANSPARENT = (0, 0, 0, 0)

ROOT = pathlib.Path(__file__).resolve().parent.parent
ICON_DIR = ROOT / "msw-app" / "icons"

# Supersampling factor. Drawing large and reducing gives clean edges without
# needing anti-aliased primitives.
SS = 8


def draw_glyph(size: int, colour: tuple[int, int, int, int], width: float = 0.84) -> Image.Image:
    """Three screens on a stand line, centred in a square of `size`.

    `width` is how much of the canvas the group of screens spans. The app icon
    leaves a margin because it sits on a rounded square; the tray icon fills
    nearly the whole canvas, because at 16 px every pixel counts.
    """
    s = size * SS
    img = Image.new("RGBA", (s, s), TRANSPARENT)
    d = ImageDraw.Draw(img)

    # Proportions as fractions of the canvas. The screens are close to 16:9 so
    # they read as monitors rather than as bars, and the group is centred
    # slightly high to leave room for the stand line.
    gap = 0.04 * s
    total_w = width * s
    screen_w = (total_w - 2 * gap) / 3
    screen_h = screen_w * 9 / 16

    left = (s - total_w) / 2
    top = (s - screen_h) / 2 - 0.05 * s
    radius = max(1, int(0.08 * screen_h))

    for i in range(3):
        x0 = left + i * (screen_w + gap)
        d.rounded_rectangle(
            [x0, top, x0 + screen_w, top + screen_h],
            radius=radius,
            fill=colour,
        )

    # Stand line beneath the screens, slightly narrower than the group.
    bar_h = 0.05 * s
    bar_w = total_w * 0.5
    bar_x = (s - bar_w) / 2
    bar_y = top + screen_h + 0.085 * s
    d.rounded_rectangle(
        [bar_x, bar_y, bar_x + bar_w, bar_y + bar_h],
        radius=bar_h / 2,
        fill=colour,
    )

    return img.resize((size, size), Image.LANCZOS)


def app_icon(size: int) -> Image.Image:
    """White glyph on an accent rounded square."""
    s = size * SS
    img = Image.new("RGBA", (s, s), TRANSPARENT)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=int(0.22 * s), fill=ACCENT)
    base = img.resize((size, size), Image.LANCZOS)

    glyph = draw_glyph(size, WHITE)
    return Image.alpha_composite(base, glyph)


def tray_icon(size: int) -> Image.Image:
    """Accent glyph on transparency, for the notification area."""
    return draw_glyph(size, ACCENT, width=0.98)


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)

    # Sizes Tauri's Windows bundler expects to find.
    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ]:
        app_icon(size).save(ICON_DIR / name)
        print(f"wrote {name}")

    # Multi-resolution .ico for the executable and installer.
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    app_icon(256).save(
        ICON_DIR / "icon.ico",
        sizes=[(n, n) for n in ico_sizes],
    )
    print("wrote icon.ico")

    # Tray icons. Windows renders the notification area at a few different
    # scales depending on DPI, so ship the common ones.
    for size in (16, 24, 32, 48):
        tray_icon(size).save(ICON_DIR / f"tray-{size}.png")
        print(f"wrote tray-{size}.png")

    tray_icon(32).save(ICON_DIR / "tray.png")
    print("wrote tray.png")


if __name__ == "__main__":
    main()
