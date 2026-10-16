#!/usr/bin/env python3
"""Generate samples/calibration_chart.png (1920x1080) + chart_layout.json.

Import the PNG into NodeVideo at 1:1 (project 1920x1080), put the Adjustment
Layer above it, export a lossless frame; also export once WITHOUT the layer.
The patch map in chart_layout.json lets us compare exact pixel positions.
"""
import colorsys
import json
import numpy as np
from PIL import Image

W, H = 1920, 1080
BG = 20
img = np.full((H, W, 3), BG, dtype=np.uint8)
layout = {"width": W, "height": H, "regions": {}}


def rect(x, y, w, h, rgb):
    img[y:y + h, x:x + w] = rgb


# 1) grey ramp (256 steps, 7 px each)
x0, y0 = 64, 40
for i in range(256):
    rect(x0 + i * 7, y0, 7, 70, (i, i, i))
layout["regions"]["gray_ramp"] = {"x": x0, "y": y0, "step_w": 7, "h": 70, "steps": 256}

# 2) R, G, B ramps
for k in range(3):
    for i in range(256):
        c = [0, 0, 0]
        c[k] = i
        rect(x0 + i * 7, 120 + k * 30, 7, 30, tuple(c))
layout["regions"]["rgb_ramps"] = {"x": x0, "y": 120, "step_w": 7, "row_h": 30, "steps": 256}

# 3) 9x9x9 colour cube, 20 px patches, 27 columns
cx, cy, ps = 64, 240, 20
levels = [round(i / 8 * 255) for i in range(9)]
n = 0
for r in levels:
    for g in levels:
        for b in levels:
            col, row = n % 27, n // 27
            rect(cx + col * ps, cy + row * ps, ps, ps, (r, g, b))
            n += 1
layout["regions"]["color_cube"] = {"x": cx, "y": cy, "patch": ps, "cols": 27, "order": "r,g,b nested, levels i/8*255", "levels": levels}

# 4) hue x saturation grid (36 hues, 4 sats at V=1.0 then 4 sats at V=0.5)
hx, hy, hs = 660, 240, 32
rows = [(1.0, 1.0), (0.75, 1.0), (0.5, 1.0), (0.25, 1.0), (1.0, 0.5), (0.75, 0.5), (0.5, 0.5), (0.25, 0.5)]
for ri, (s, v) in enumerate(rows):
    for hi in range(36):
        r, g, b = colorsys.hsv_to_rgb(hi / 36, s, v)
        rect(hx + hi * hs, hy + ri * hs, hs, hs, (round(r * 255), round(g * 255), round(b * 255)))
layout["regions"]["hue_grid"] = {"x": hx, "y": hy, "patch": hs, "hues": 36, "rows_sv": rows}

# 5) memory colours
mem = [(194, 150, 130), (141, 85, 36), (255, 224, 189), (86, 140, 200), (90, 140, 60), (200, 60, 50),
       (240, 200, 60), (120, 120, 120), (235, 235, 235), (50, 50, 50), (150, 110, 180), (60, 160, 160)]
for i, c in enumerate(mem):
    rect(660 + i * 96, 510, 96, 36, c)
layout["regions"]["memory_colors"] = {"x": 660, "y": 510, "patch_w": 96, "h": 36, "colors": mem}

# 6) glow test on black
rect(660, 570, 1200, 210, (0, 0, 0))
glow_levels = [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]
for i, v in enumerate(glow_levels):
    c = round(v * 255)
    rect(690 + i * 80, 600, 50, 50, (c, c, c))
sat = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0), (0, 255, 255), (255, 0, 255), (255, 160, 0)]
for i, c in enumerate(sat):
    rect(690 + i * 80, 710, 50, 50, c)
dots = [2, 4, 8, 16, 32]
dx = 1430
for d in dots:
    rect(dx, 675 - d // 2, d, d, (255, 255, 255))
    dx += d + 40
layout["regions"]["glow_test"] = {"black_box": [660, 570, 1200, 210], "gray_squares": {"x0": 690, "y": 600, "size": 50, "pitch": 80, "levels": glow_levels},
                                  "color_squares": {"x0": 690, "y": 710, "size": 50, "pitch": 80, "colors": sat}, "white_dots": {"x0": 1430, "centre_y": 675, "sizes": dots}}

# 7) sharpness: stripe blocks of increasing frequency
sx, sy = 64, 820
for bi, period in enumerate([2, 4, 6, 8, 12, 16]):
    bx = sx + bi * 190
    for xx in range(0, 180):
        v = 204 if (xx // (period // 2)) % 2 == 0 else 51
        img[sy:sy + 120, bx + xx] = (v, v, v)
layout["regions"]["stripes"] = {"x": sx, "y": sy, "block_w": 180, "pitch": 190, "h": 120, "periods_px": [2, 4, 6, 8, 12, 16], "values": [204, 51]}

# slanted edge (5 degrees) between 25 and 230
ex, ey, es = 1260, 820, 200
yy, xx = np.mgrid[0:es, 0:es]
mask = (xx - es / 2) * np.cos(np.radians(5)) - (yy - es / 2) * np.sin(np.radians(5)) > 0
patch = np.where(mask, 230, 25).astype(np.uint8)
img[ey:ey + es, ex:ex + es] = patch[..., None]
layout["regions"]["slanted_edge"] = {"x": ex, "y": ey, "size": es, "angle_deg": 5, "values": [25, 230]}

# flat mid grey and a few flat tones
for i, v in enumerate([64, 128, 192]):
    rect(1500 + i * 120, 820, 100, 100, (v, v, v))
layout["regions"]["flat_tones"] = {"x": 1500, "y": 820, "size": 100, "pitch": 120, "values": [64, 128, 192]}

# 8) corner markers so the 1:1 mapping can be verified (white 8x8 squares)
for (mx, my) in [(0, 0), (W - 8, 0), (0, H - 8), (W - 8, H - 8)]:
    rect(mx, my, 8, 8, (255, 255, 255))

Image.fromarray(img, "RGB").save("samples/calibration_chart.png", optimize=True)
json.dump(layout, open("samples/chart_layout.json", "w"), indent=1)
print("ok", img.shape)
