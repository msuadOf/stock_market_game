#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
# How to run: python3 scripts/desktop/validate-wayland-png.py <capture.png> <width> <height>

from collections import deque
from pathlib import Path
from sys import argv

from PIL import Image, ImageStat


def validate(capture: Path, expected_width: int, expected_height: int) -> None:
    with Image.open(capture) as source:
        source.verify()
    with Image.open(capture) as source:
        image = source.convert("RGBA")

    if image.size != (expected_width, expected_height):
        raise SystemExit(f"unexpected dimensions: {image.size}")
    pixels = list(image.get_flattened_data())
    if len(set(pixels[::97])) < 32:
        raise SystemExit("insufficient sampled pixel variance")
    if all(red == green == blue == 0 for red, green, blue, _alpha in pixels):
        raise SystemExit("capture is entirely black")
    if all(alpha == 0 for _red, _green, _blue, alpha in pixels):
        raise SystemExit("capture is entirely transparent")

    width, height = image.size
    bright = [red > 235 and green > 235 and blue > 235 and alpha > 0 for red, green, blue, alpha in pixels]
    visited = bytearray(len(bright))
    components: list[tuple[int, int, int, int, int]] = []
    for start, enabled in enumerate(bright):
        if not enabled or visited[start]:
            continue
        visited[start] = 1
        queue = deque([start])
        count = 0
        min_x = max_x = start % width
        min_y = max_y = start // width
        while queue:
            current = queue.popleft()
            count += 1
            x = current % width
            y = current // width
            min_x = min(min_x, x)
            max_x = max(max_x, x)
            min_y = min(min_y, y)
            max_y = max(max_y, y)
            neighbors = (
                current - 1 if x else -1,
                current + 1 if x + 1 < width else -1,
                current - width if y else -1,
                current + width if y + 1 < height else -1,
            )
            for neighbor in neighbors:
                if neighbor >= 0 and bright[neighbor] and not visited[neighbor]:
                    visited[neighbor] = 1
                    queue.append(neighbor)
        if count >= 20_000:
            components.append((count, min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))

    app_regions = [component for component in components if component[2] >= 90 and component[3] >= 250 and component[4] >= 150]
    if not app_regions:
        raise SystemExit("no app-visible bright region below the shell panel")

    stat = ImageStat.Stat(image)
    print(
        {
            "capture": str(capture),
            "width": width,
            "height": height,
            "mode": image.mode,
            "mean": [round(value, 2) for value in stat.mean],
            "alpha_extrema": image.getextrema()[3],
            "sampled_unique_colors": len(set(pixels[::97])),
            "bright_fraction": round(sum(bright) / len(bright), 6),
            "app_regions": app_regions,
        }
    )


if len(argv) != 4:
    raise SystemExit("usage: validate-wayland-png.py <capture.png> <width> <height>")

validate(Path(argv[1]), int(argv[2]), int(argv[3]))
