#!/usr/bin/env python3
"""Generate the application and tray icons.

Kept in the repo so the icons can be regenerated rather than treated as opaque
binaries. Run from the repository root:

    python tools/make_icons.py

Design notes:

* The glyph is a single monitor: a solid screen, a neck and a base. Solid
  shapes rather than outlines, because an outline that looks right at 512 px
  vanishes at 16 px, and the tray is where this icon has to earn its keep.
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


def draw_glyph(size: int, colour: tuple[int, int, int, int], width: float = 0.76) -> Image.Image:
    """One monitor — screen, neck, base — centred in a square of `size`.

    `width` is how much of the canvas the screen spans. The app icon leaves a
    margin because it sits on a rounded square; the tray icon fills more of its
    canvas, because at 16 px every pixel counts.

    The screen is a solid rounded rectangle rather than an outlined bezel. An
    outline thin enough to look right at 512 px disappears at 16 px, and the
    tray is where this icon actually has to work.
    """
    s = size * SS
    img = Image.new("RGBA", (s, s), TRANSPARENT)
    d = ImageDraw.Draw(img)

    screen_w = width * s
    screen_h = screen_w * 0.66  # a little taller than 16:9, which reads better small

    neck_w = screen_w * 0.17
    neck_h = s * 0.075
    base_w = screen_w * 0.46
    base_h = s * 0.058

    # Centre the whole assembly vertically.
    total_h = screen_h + neck_h + base_h
    top = (s - total_h) / 2
    cx = s / 2

    d.rounded_rectangle(
        [cx - screen_w / 2, top, cx + screen_w / 2, top + screen_h],
        radius=max(1, int(0.10 * screen_h)),
        fill=colour,
    )

    neck_top = top + screen_h
    d.rectangle(
        [cx - neck_w / 2, neck_top, cx + neck_w / 2, neck_top + neck_h],
        fill=colour,
    )

    base_top = neck_top + neck_h
    d.rounded_rectangle(
        [cx - base_w / 2, base_top, cx + base_w / 2, base_top + base_h],
        radius=base_h / 2,
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
    return draw_glyph(size, ACCENT, width=0.84)


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
