#!/usr/bin/env python3
"""Compare the GPUI initial frame with the audited Leptos reference golden."""
import sys
from PIL import Image, ImageChops, ImageStat

if len(sys.argv) != 3:
    raise SystemExit("usage: compare_pixels.py REFERENCE.png ACTUAL.png")
reference = Image.open(sys.argv[1]).convert("RGB")
actual = Image.open(sys.argv[2]).convert("RGB")
# Some hosted Xvfb/Vulkan combinations expose a live, interactive WebGPU surface
# but omit that direct-present surface from both CDP and X11 screenshots. Detect
# that driver limitation explicitly rather than treating the background-only frame
# as a visual regression; the browser interaction/fingerprint assertions still run.
colors = actual.getcolors(maxcolors=actual.width * actual.height)
if colors and max(count for count, _ in colors) / (actual.width * actual.height) > 0.98:
    raise SystemExit("WebGPU surface is absent from the captured parity frame")
if reference.size != actual.size:
    raise SystemExit(f"screenshot dimensions diverged: reference={reference.size}, actual={actual.size}")
width, height = reference.size
# Ignore the browser compositor's one-pixel separator consistently in both frames.
reference = reference.crop((0, 1, width, height))
actual = actual.crop((0, 1, width, height))
height -= 1
difference = ImageChops.difference(reference, actual)
stat = ImageStat.Stat(difference)
mae = sum(stat.mean) / 3.0
ref_pixels = reference.load()
actual_pixels = actual.load()
exact = 0
large = 0
total = width * height
for y in range(height):
    for x in range(width):
        left = ref_pixels[x, y]
        right = actual_pixels[x, y]
        delta = max(abs(left[channel] - right[channel]) for channel in range(3))
        exact += left == right
        large += delta > 20
exact_ratio = exact / total
large_ratio = large / total
print(f"pixel parity: size={width}x{height} mae={mae:.4f} exact={exact_ratio:.4%} delta>20={large_ratio:.4%} "
      f"reference-bg={reference.getpixel((0, 0))} actual-bg={actual.getpixel((0, 0))}")
# Font rasterization and GPU antialiasing differ slightly, but structural divergence is
# deliberately given very little room. The worst audited state (catalog) is
# ~1.48 / 87.2% / 2.0%; the other three states are substantially closer.
if mae > 1.65 or exact_ratio < 0.86 or large_ratio > 0.022:
    raise SystemExit("initial frame diverged from the audited Leptos golden")
