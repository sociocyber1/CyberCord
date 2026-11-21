#!/usr/bin/env python3
"""Small animated GIF stickers on general-semantics themes (GNOME style)."""
import io, math, os, subprocess
from PIL import Image
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(root, "assets", "gifs"); os.makedirs(OUT, exist_ok=True)
B, B1, B5, G, G5, O, P, R, Y, BR = "#3584e4", "#99c1f1", "#1a5fb4", "#2ec27e", "#26a269", "#ff7800", "#9141ac", "#e01b24", "#f6d32d", "#865e3c"
INK, L1, L3, L4 = "#241f31", "#ffffff", "#deddda", "#c0bfbc"
F = "Inter, Cantarell, sans-serif"
S = 200

def frame(body):
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="{S}" height="{S}" viewBox="0 0 {S} {S}"><rect width="{S}" height="{S}" fill="#eaf2fd"/>{body}</svg>'
    png = subprocess.run(["rsvg-convert"], input=svg.encode(), capture_output=True, check=True).stdout
    return Image.open(io.BytesIO(png)).convert("RGBA")

def person(x, y, s, c):
    return f'<circle cx="{x}" cy="{y-9*s}" r="{6*s}" fill="{c}"/><path d="M{x-10*s} {y+12*s} v-4 a{10*s} {9*s} 0 0 1 {20*s} 0 v4 z" fill="{c}"/>'

def save(name, frames, ms):
    pal = [f.convert("RGB").quantize(colors=64, method=Image.Quantize.MEDIANCUT) for f in frames]
    pal[0].save(os.path.join(OUT, name + ".gif"), save_all=True, append_images=pal[1:], duration=ms, loop=0, optimize=True, disposal=2)

# 1. map ≠ territory: label on the map, then the real many-sided person
fr = []
for k in range(12):
    t = k / 11
    a = 1 - min(1, max(0, (t - 0.35) * 3))
    body = f'<g opacity="{a:.2f}"><path d="M30 60 L70 46 L110 60 L150 46 V140 L110 154 L70 140 L30 154 Z" fill="#f9f06b" stroke="{BR}" stroke-width="4" stroke-linejoin="round"/><rect x="58" y="88" width="64" height="22" rx="7" fill="{R}"/><rect x="68" y="96" width="44" height="6" rx="3" fill="#fff"/></g>'
    b2 = 1 - a
    rays = ''.join(f'<line x1="100" y1="96" x2="{100+60*math.cos(q):.0f}" y2="{96+52*math.sin(q):.0f}" stroke="{c}" stroke-width="4" stroke-dasharray="3 7" stroke-linecap="round"/><circle cx="{100+60*math.cos(q):.0f}" cy="{96+52*math.sin(q):.0f}" r="10" fill="{c}"/>' for q, c in [(-2.5, G5), (-1.6, P), (-0.6, O), (0.3, BR)])
    body += f'<g opacity="{b2:.2f}"><path d="M10 190 C50 120 90 110 100 130 C120 110 160 120 190 190 Z" fill="#57e389"/>{rays}{person(100, 140, 2.0, B)}</g>'
    fr.append(frame(body))
save("map-territory", fr, [160] * 3 + [90] * 6 + [900] + [160] * 2)

# 2. pause: breathe in, breathe out (delayed reaction)
fr = []
for k in range(16):
    r = 38 + 28 * (0.5 - 0.5 * math.cos(2 * math.pi * k / 16))
    body = f'<circle cx="100" cy="100" r="{r+18:.1f}" fill="{B1}" opacity=".35"/><circle cx="100" cy="100" r="{r:.1f}" fill="{B}"/>'
    body += f'<rect x="88" y="80" width="9" height="40" rx="3" fill="#fff"/><rect x="104" y="80" width="9" height="40" rx="3" fill="#fff"/>'
    fr.append(frame(body))
save("pause", fr, 110)

# 3. indexing: Ivan₁, Ivan₂, Ivan₃ appear — not the same person
fr = []
cols = [B, P, G5]
for k in range(10):
    body = ''
    n = min(3, 1 + k // 3)
    for i in range(n):
        x = 45 + i * 55
        body += person(x, 110, 1.9, cols[i]) + f'<text x="{x}" y="176" font-family="{F}" font-size="30" font-weight="800" fill="{cols[i]}" text-anchor="middle">{"₁₂₃"[i]}</text>'
    if n > 1:
        body += f'<text x="72" y="118" font-family="{F}" font-size="28" font-weight="800" fill="{INK}" text-anchor="middle">≠</text>'
    if n > 2:
        body += f'<text x="127" y="118" font-family="{F}" font-size="28" font-weight="800" fill="{INK}" text-anchor="middle">≠</text>'
    fr.append(frame(body))
save("index", fr, [500] * 9 + [1200])

# 4. build bridges: planks appear between two people
fr = []
for k in range(12):
    n = min(9, k)
    body = f'<path d="M0 200 V130 Q30 124 56 128 L64 200 Z" fill="#57e389"/><path d="M200 200 V130 Q170 124 144 128 L136 200 Z" fill="#57e389"/><path d="M60 200 Q100 170 140 200 Z" fill="#62a0ea"/>'
    for i in range(n):
        x = 60 + (i + 0.5) * 80 / 9
        y = 128 - 18 * math.sin(math.pi * (i + 0.5) / 9)
        body += f'<rect x="{x-4:.1f}" y="{y:.1f}" width="8" height="12" rx="2" fill="{BR}"/>'
    body += person(32, 104, 1.3, B) + person(168, 104, 1.3, P)
    if k >= 10:
        body += f'<rect x="76" y="40" width="48" height="30" rx="10" fill="#fff"/><circle cx="90" cy="55" r="4" fill="{G5}"/><circle cx="100" cy="55" r="4" fill="{G5}"/><circle cx="110" cy="55" r="4" fill="{G5}"/>'
    fr.append(frame(body))
save("bridge", fr, [220] * 10 + [600, 1200])

# 5. check the facts: magnifier passes over lines, then a check mark
fr = []
for k in range(12):
    body = f'<rect x="30" y="40" width="140" height="120" rx="14" fill="#fff"/>' + ''.join(f'<rect x="48" y="{62+i*22}" width="{100 - (i % 2) * 30}" height="9" rx="4.5" fill="{L4}"/>' for i in range(4))
    if k < 9:
        x = 60 + 80 * (k / 8)
        y = 70 + 50 * abs(math.sin(k / 8 * math.pi))
        body += f'<circle cx="{x:.0f}" cy="{y:.0f}" r="22" fill="#fff" fill-opacity=".4" stroke="{B5}" stroke-width="7"/><path d="M{x+16:.0f} {y+16:.0f} l18 18" stroke="{B5}" stroke-width="10" stroke-linecap="round"/>'
    else:
        body += f'<circle cx="150" cy="150" r="30" fill="{G5}"/><path d="M135 150 l10 10 l20 -20" stroke="#fff" stroke-width="8" fill="none" stroke-linecap="round" stroke-linejoin="round"/>'
    fr.append(frame(body))
save("check-facts", fr, [140] * 9 + [500, 500, 1200])
print("ok")
