#!/usr/bin/env python3
"""GNOME-style illustrations for the in-app documents (SVG + PNG)."""
import math, os, subprocess
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(root, "docs", "img"); AS = os.path.join(root, "assets", "docs")
os.makedirs(OUT, exist_ok=True); os.makedirs(AS, exist_ok=True)
BLUE, BLUE1, BLUE5 = "#3584e4", "#99c1f1", "#1a5fb4"
GREEN, GREEN5, RED, YEL, PURP, ORANGE, BROWN = "#2ec27e", "#26a269", "#e01b24", "#f6d32d", "#9141ac", "#ff7800", "#865e3c"
INK, INK2, L1, L2, L3, L4 = "#241f31", "#77767b", "#ffffff", "#f6f5f4", "#deddda", "#c0bfbc"

def svg(body, w=560, h=300, sky=True):
    bg = f'<rect width="{w}" height="{h}" rx="20" fill="url(#sky)"/><circle cx="80" cy="260" r="100" fill="{BLUE1}" opacity=".16"/><circle cx="{w-60}" cy="40" r="80" fill="{BLUE1}" opacity=".14"/>' if sky else ''
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<defs><filter id="sh" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#000" flood-opacity=".13"/></filter>
<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#eaf2fd"/><stop offset="1" stop-color="#f6f5f4"/></linearGradient>
<linearGradient id="hill" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#8ff0a4"/><stop offset="1" stop-color="#33d17a"/></linearGradient>
<linearGradient id="earth" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#62a0ea"/><stop offset="1" stop-color="#1a5fb4"/></linearGradient>
<linearGradient id="water" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#99c1f1"/><stop offset="1" stop-color="#62a0ea"/></linearGradient>
<clipPath id="earthclip"><circle cx="400" cy="150" r="88"/></clipPath>
</defs>{bg}{body}</svg>'''

def person(cx, cy, s, c):
    return (f'<circle cx="{cx}" cy="{cy-26*s}" r="{14*s}" fill="{c}"/>'
            f'<path d="M{cx-24*s} {cy+30*s} v-14 a{24*s} {22*s} 0 0 1 {48*s} 0 v14 z" fill="{c}"/>')

imgs = {}

# 1. The map is not the territory: a word-label vs a real, many-sided person
b = f'<g filter="url(#sh)"><rect x="40" y="70" width="190" height="150" rx="10" fill="{L1}" stroke="{L3}"/></g>'
b += f'<path d="M58 92 L100 82 L150 92 L210 82 V200 L150 210 L100 200 L58 210 Z" fill="#f9f06b" opacity=".35" stroke="{L4}"/>'
b += f'<path d="M100 82 V200 M150 92 V210" stroke="{L4}" stroke-width="2"/>'
b += f'<rect x="78" y="128" width="112" height="34" rx="8" fill="{RED}" opacity=".9"/><rect x="92" y="141" width="84" height="8" rx="4" fill="#fff"/>'
b += f'<path d="M248 148 h40 M248 162 h40" stroke="{INK2}" stroke-width="4" stroke-linecap="round"/><path d="M262 176 L276 134" stroke="{RED}" stroke-width="4" stroke-linecap="round"/>'
b += f'<path d="M300 228 C330 150 370 130 400 160 C430 120 480 110 530 200 V228 Z" fill="url(#hill)"/>'
b += f'<circle cx="470" cy="80" r="22" fill="{YEL}"/>'
b += person(410, 190, 1.4, BLUE)
for (x, y, c) in [(360, 92, GREEN), (440, 60, PURP), (505, 128, ORANGE), (330, 150, BROWN)]:
    b += f'<line x1="410" y1="150" x2="{x}" y2="{y}" stroke="{c}" stroke-width="2.5" stroke-dasharray="3 6" stroke-linecap="round"/><circle cx="{x}" cy="{y}" r="11" fill="{c}"/>'
imgs["words-map"] = b

# 2. Build bridges: two people, a bridge being completed between two cliffs
b = f'<path d="M0 300 V190 Q60 175 150 185 L170 300 Z" fill="url(#hill)"/><path d="M560 300 V190 Q500 175 410 185 L390 300 Z" fill="url(#hill)"/>'
b += f'<path d="M150 300 Q280 250 410 300 Z" fill="url(#water)"/>'
b += f'<path d="M150 186 Q280 140 410 186" fill="none" stroke="{BROWN}" stroke-width="8" stroke-linecap="round"/>'
for i in range(9):
    t = (i + 1) / 10
    x = 150 + 260 * t; y = (1 - t) ** 2 * 186 + 2 * (1 - t) * t * 140 + t * t * 186
    col = BROWN if i not in (4,) else YEL
    b += f'<line x1="{x:.1f}" y1="{y:.1f}" x2="{x:.1f}" y2="{y+24:.1f}" stroke="{col}" stroke-width="5" stroke-linecap="round"/>'
b += f'<path d="M150 210 Q280 164 410 210" fill="none" stroke="{BROWN}" stroke-width="5" stroke-linecap="round" opacity=".8"/>'
b += person(100, 160, 1.15, BLUE) + person(460, 160, 1.15, PURP)
b += f'<g transform="translate(250,70)" filter="url(#sh)"><rect width="60" height="40" rx="12" fill="{L1}" stroke="{L3}"/><path d="M14 40 l-6 12 l16 -12" fill="{L1}"/><circle cx="18" cy="20" r="4" fill="{GREEN}"/><circle cx="30" cy="20" r="4" fill="{GREEN}"/><circle cx="42" cy="20" r="4" fill="{GREEN}"/></g>'
imgs["words-bridge"] = b

# 3. Escalation: dominoes from a word to a broken network
b = ''
labels = [RED, ORANGE, YEL, PURP, INK2]
for i, c in enumerate(labels):
    x = 60 + i * 92; ang = min(i * 14, 62)
    b += f'<g transform="translate({x},230) rotate({ang})"><rect x="-14" y="-110" width="28" height="110" rx="5" fill="{c}" filter="url(#sh)"/><circle cx="0" cy="-86" r="4" fill="#fff"/><circle cx="0" cy="-60" r="4" fill="#fff"/></g>'
b += f'<line x1="30" y1="232" x2="530" y2="232" stroke="{L4}" stroke-width="3"/>'
b += f'<g transform="translate(70,58)" filter="url(#sh)"><rect width="70" height="34" rx="11" fill="{L1}" stroke="{RED}" stroke-width="2"/><path d="M16 34 l-5 10 l14 -10" fill="{L1}" stroke="{RED}" stroke-width="2"/><path d="M22 12 l26 10 M48 12 l-26 10" stroke="{RED}" stroke-width="3" stroke-linecap="round"/></g>'
# broken network at the end
nodes = [(470, 70), (520, 110), (450, 130), (505, 165)]
for (a, c) in [((470,70),(520,110)), ((450,130),(505,165))]:
    b += f'<line x1="{a[0]}" y1="{a[1]}" x2="{c[0]}" y2="{c[1]}" stroke="{INK2}" stroke-width="2.5"/>'
b += f'<path d="M478 95 l10 10 M488 95 l-10 10" stroke="{RED}" stroke-width="3" stroke-linecap="round"/>'
for (x, y) in nodes:
    b += f'<circle cx="{x}" cy="{y}" r="11" fill="{L4}" stroke="{INK2}" stroke-width="2"/>'
imgs["consequences-chain"] = b

# 4. The planet pays — or cooperates: two halves of one Earth
b = f'<g filter="url(#sh)"><circle cx="400" cy="150" r="88" fill="url(#earth)"/></g>'
b += f'<g clip-path="url(#earthclip)"><path d="M330 110 q30 -30 60 -10 q20 15 50 0 q25 -10 30 20 q-20 30 -60 20 q-30 -10 -50 15 q-20 15 -40 -10 z" fill="{GREEN}"/><path d="M360 190 q30 -15 60 5 q15 20 -10 35 q-30 10 -50 -10 z" fill="{GREEN}"/></g>'
b += f'<path d="M400 62 V238" stroke="#fff" stroke-width="3" stroke-dasharray="6 6" opacity=".8"/>'
for k in range(6):
    a = math.pi / 2 + (k - 2.5) * 0.35
    b += f'<circle cx="{400 + 118*math.cos(a+math.pi/2+1.57):.1f}" cy="{150 - 118*math.sin(a):.1f}" r="0" />'
# left: shared work (hands of many people around), right: conflicts drain resources
for i, c in enumerate([BLUE, GREEN5, PURP, ORANGE, BROWN]):
    a = math.pi * (-0.36 + i * 0.18)
    b += person(400 + 128 * math.cos(a), 150 - 128 * math.sin(a) + 26, 0.55, c)
b += f'<g transform="translate(60,70)"><rect width="190" height="160" rx="14" fill="{L1}" stroke="{L3}" filter="url(#sh)"/>'
for i, (w, c) in enumerate([(150, GREEN), (120, BLUE), (90, ORANGE), (40, RED)]):
    b += f'<rect x="20" y="{24 + i*32}" width="{w}" height="14" rx="7" fill="{c}"/>'
b += '</g>'
b += f'<path d="M262 150 h40 l-10 -10 m10 10 l-10 10" stroke="{INK2}" stroke-width="3" fill="none" stroke-linecap="round"/>'
imgs["consequences-planet"] = b

# 5. SocioCyberneering: a circular city linked by a node network
b = f'<g transform="translate(180,150)">'
b += f'<circle r="118" fill="url(#hill)" opacity=".55"/>'
for r, c in [(96, L1), (70, "#dcebfb"), (44, L1)]:
    b += f'<circle r="{r}" fill="{c}" stroke="{BLUE1}" stroke-width="2"/>'
for k in range(12):
    a = 2 * math.pi * k / 12
    b += f'<line x1="{44*math.cos(a):.1f}" y1="{44*math.sin(a):.1f}" x2="{96*math.cos(a):.1f}" y2="{96*math.sin(a):.1f}" stroke="{BLUE1}" stroke-width="2"/>'
    b += f'<rect x="{82*math.cos(a)-5:.1f}" y="{82*math.sin(a)-5:.1f}" width="10" height="10" rx="2" fill="{GREEN}" />'
b += f'<circle r="26" fill="{BLUE}"/><circle r="12" fill="#fff"/><circle r="5" fill="{BLUE}"/></g>'
pts = [(380, 80), (470, 110), (430, 190), (520, 200), (360, 230)]
for (a, c) in [(0,1),(1,2),(2,3),(0,2),(2,4),(1,3)]:
    b += f'<line x1="{pts[a][0]}" y1="{pts[a][1]}" x2="{pts[c][0]}" y2="{pts[c][1]}" stroke="{BLUE}" stroke-width="2.5" stroke-dasharray="2 6" stroke-linecap="round"/>'
b += f'<line x1="290" y1="150" x2="380" y2="80" stroke="{BLUE}" stroke-width="2.5" stroke-dasharray="2 6" stroke-linecap="round"/><line x1="290" y1="160" x2="360" y2="230" stroke="{BLUE}" stroke-width="2.5" stroke-dasharray="2 6" stroke-linecap="round"/>'
for (x, y), c in zip(pts, [GREEN5, PURP, BLUE5, ORANGE, BROWN]):
    b += f'<circle cx="{x}" cy="{y}" r="15" fill="{c}"/><circle cx="{x}" cy="{y-4}" r="5" fill="#fff"/><path d="M{x-9} {y+9} a9 7 0 0 1 18 0" fill="#fff"/>'
imgs["sce-city"] = b

# 6. Structural differential: event (parabola) → object (disk) → labels
b = ''
cx = 170
holes = [(cx + dx, 58 + dy) for dx, dy in [(-60, 6), (-38, 14), (-15, 4), (8, 12), (30, 5), (52, 13), (-48, 30), (-22, 26), (2, 32), (26, 28), (46, 34), (-30, 48), (-6, 52), (18, 50), (38, 56), (-12, 72), (12, 74)]]
b += f'<path d="M{cx-92} 40 Q{cx} 190 {cx+92} 40" fill="#dcebfb" stroke="{BLUE}" stroke-width="3"/>'
b += f'<path d="M{cx-92} 40 h184" stroke="{BLUE}" stroke-width="3" stroke-dasharray="4 7" opacity=".7"/>'
for (x, y) in holes:
    b += f'<circle cx="{x}" cy="{y}" r="3.2" fill="#fff" stroke="{BLUE1}"/>'
disk = (cx, 152)
b += f'<ellipse cx="{disk[0]}" cy="{disk[1]}" rx="46" ry="14" fill="{L3}" stroke="{INK2}" stroke-width="2.5"/>'
for (x, y) in [holes[1], holes[8], holes[11], holes[15]]:
    b += f'<line x1="{x}" y1="{y}" x2="{x*0.4+disk[0]*0.6:.1f}" y2="{disk[1]-6}" stroke="{INK2}" stroke-width="1.4"/>'
tags = [(GREEN5, 192), (ORANGE, 216), (RED, 240)]
prev = (disk[0], disk[1] + 12)
for i, (col, y) in enumerate(tags):
    x = cx - 34 + i * 10
    b += f'<line x1="{prev[0]}" y1="{prev[1]}" x2="{x+34}" y2="{y-6}" stroke="{INK2}" stroke-width="1.4"/>'
    b += f'<rect x="{x}" y="{y-6}" width="68" height="14" rx="4" fill="{col}"/><circle cx="{x+60}" cy="{y+1}" r="2.5" fill="#fff"/>'
    prev = (x + 34, y + 8)
b += f'<path d="M{cx+48} 246 C{cx+150} 240 {cx+150} 80 {cx+80} 70" fill="none" stroke="{INK2}" stroke-width="1.6" stroke-dasharray="5 6"/>'
# right: three chat bubbles mapped to the three levels
for i, (col, w) in enumerate([(GREEN5, 170), (ORANGE, 140), (RED, 110)]):
    y = 70 + i * 62
    b += f'<g filter="url(#sh)"><rect x="350" y="{y}" width="{w+40}" height="44" rx="14" fill="{L1}"/></g><rect x="362" y="{y+10}" width="14" height="24" rx="4" fill="{col}"/>'
    b += f'<rect x="386" y="{y+12}" width="{w}" height="7" rx="3.5" fill="{L4}"/><rect x="386" y="{y+25}" width="{w-40}" height="7" rx="3.5" fill="{L3}"/>'
imgs["structural-differential"] = b

# 7. Atom-Generator: stars (H, He) → elements (C, N, O) → plant / bioreactor → food
b = ''
b += f'<circle cx="80" cy="120" r="44" fill="{YEL}"/><circle cx="80" cy="120" r="58" fill="{YEL}" opacity=".25"/>'
for k in range(10):
    a = 2 * math.pi * k / 10
    b += f'<line x1="{80+50*math.cos(a):.1f}" y1="{120+50*math.sin(a):.1f}" x2="{80+66*math.cos(a):.1f}" y2="{120+66*math.sin(a):.1f}" stroke="{ORANGE}" stroke-width="4" stroke-linecap="round"/>'
for (x, y, col) in [(70, 112, BLUE), (92, 130, PURP)]:
    b += f'<circle cx="{x}" cy="{y}" r="9" fill="{col}"/>'
arrow = lambda x: f'<path d="M{x} 120 h34 m-10 -9 l10 9 l-10 9" stroke="{INK2}" stroke-width="4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>'
b += arrow(150)
for i, col in enumerate([INK, BLUE, RED]):
    y = 80 + i * 40
    b += f'<circle cx="230" cy="{y}" r="15" fill="{col}"/><circle cx="230" cy="{y}" r="5" fill="#fff"/>'
    b += f'<ellipse cx="230" cy="{y}" rx="26" ry="9" fill="none" stroke="{col}" stroke-width="2" opacity=".6"/>'
b += arrow(272)
b += f'<rect x="322" y="70" width="70" height="110" rx="14" fill="#dcebfb" stroke="{BLUE}" stroke-width="3"/><path d="M322 130 h70 v36 a14 14 0 0 1 -14 14 h-42 a14 14 0 0 1 -14 -14 z" fill="{GREEN}" opacity=".7"/>'
for k in range(6):
    b += f'<circle cx="{338 + (k % 3) * 18}" cy="{100 + (k // 3) * 16}" r="4" fill="{BLUE1}"/>'
b += f'<path d="M357 60 v-14 m-8 6 h16" stroke="{INK2}" stroke-width="3" stroke-linecap="round"/>'
b += arrow(404)
b += f'<ellipse cx="490" cy="150" rx="44" ry="14" fill="{L3}"/><path d="M452 146 q38 -70 76 0 z" fill="{ORANGE}"/><circle cx="476" cy="118" r="9" fill="{RED}"/><circle cx="498" cy="112" r="10" fill="{GREEN}"/><path d="M490 102 q6 -14 16 -16" stroke="{GREEN5}" stroke-width="3" fill="none"/>'
imgs["atom-generator"] = b

# 8. Hypothesis 1: the nucleus bobs like a ball on a river, its ripples hold the
#    electron like a surfer in a trough; neighbouring atoms add their own waves.
#    Inset: an orbit that fits a whole number of wavelengths (de Broglie).
b = f'<rect x="0" y="120" width="560" height="180" rx="0" fill="url(#water)" opacity=".55"/>'
b += f'<path d="M0 120 Q35 112 70 120 T140 120 T210 120 T280 120 T350 120 T420 120 T490 120 T560 120 V136 H0 Z" fill="{BLUE1}" opacity=".7"/>'
cx, cy = 280, 196
for k in range(6, 0, -1):
    rx = 34 + k * 40; ry = rx * 0.3
    op = 0.75 - k * 0.09
    b += f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry:.1f}" fill="none" stroke="#fff" stroke-width="{5-k*0.5:.1f}" opacity="{op:.2f}"/>'
    b += f'<ellipse cx="{cx}" cy="{cy+5}" rx="{rx}" ry="{ry:.1f}" fill="none" stroke="{BLUE5}" stroke-width="2" opacity="{op*0.5:.2f}"/>'
# neighbours at the edges, their ripples meet ours
for (nx, col) in [(18, PURP), (542, GREEN)]:
    for k in range(1, 4):
        rx = 16 + k * 30
        b += f'<ellipse cx="{nx}" cy="{cy+10}" rx="{rx}" ry="{rx*0.3:.1f}" fill="none" stroke="#fff" stroke-width="2.5" opacity="{0.6-k*0.12:.2f}"/>'
    b += f'<circle cx="{nx}" cy="{cy}" r="17" fill="{col}" filter="url(#sh)"/><circle cx="{nx-5}" cy="{cy-6}" r="5" fill="#fff" opacity=".45"/>'
# the nucleus: a cluster of protons and neutrons, bobbing and spinning on the water
b += f'<ellipse cx="{cx}" cy="{cy-2}" rx="54" ry="15" fill="none" stroke="{ORANGE}" stroke-width="3" stroke-dasharray="1 0" opacity=".9"/>'
b += f'<g filter="url(#sh)">'
for (dx, dy, col) in [(-13, -6, RED), (13, -6, BLUE5), (0, -28, BLUE5), (-15, -30, RED), (15, -30, RED), (0, -6, RED), (-6, -17, BLUE5), (8, -17, RED)]:
    b += f'<circle cx="{cx+dx}" cy="{cy+dy}" r="12" fill="{col}"/><circle cx="{cx+dx-4}" cy="{cy+dy-4}" r="3.5" fill="#fff" opacity=".45"/>'
b += '</g>'
b += f'<path d="M{cx-54} {cy-2} A54 15 0 0 0 {cx+54} {cy-2}" fill="none" stroke="{ORANGE}" stroke-width="3.5"/><path d="M{cx+54} {cy-2} m-4 -10 l4 10 l-11 2" fill="none" stroke="{ORANGE}" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round"/>'
b += f'<path d="M{cx} {cy-50} v-34 m-9 9 l9 -9 l9 9 M{cx} {cy-50} m-9 -9 l9 9 l9 -9" stroke="{INK2}" stroke-width="3.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>'
# the electron: a small surfer on a board, sitting in a trough of ring 3
ex, ey = cx + 150, cy + 22
b += f'<path d="M{ex-26} {ey+12} Q{ex} {ey+20} {ex+26} {ey+12} Q{ex} {ey+6} {ex-26} {ey+12} Z" fill="{YEL}" stroke="{BROWN}" stroke-width="1.5"/>'
b += f'<circle cx="{ex}" cy="{ey-4}" r="12" fill="{BLUE}" filter="url(#sh)"/><circle cx="{ex-4}" cy="{ey-8}" r="3.5" fill="#fff" opacity=".6"/>'
b += f'<circle cx="{ex}" cy="{ey-4}" r="21" fill="none" stroke="{BLUE}" stroke-width="2" stroke-dasharray="3 4" opacity=".7"/>'
b += f'<path d="M{ex-60} {ey+4} q15 -14 30 0" stroke="#fff" stroke-width="3" fill="none" opacity=".8"/><path d="M{ex+30} {ey+4} q15 -14 30 0" stroke="#fff" stroke-width="3" fill="none" opacity=".8"/>'
# inset: standing wave wrapped around an orbit, n = 5
ix, iy, R = 478, 62, 34
pts = []
for j in range(121):
    a = 2 * math.pi * j / 120
    r = R + 6 * math.sin(5 * a)
    pts.append(f'{ix + r*math.cos(a):.1f},{iy + r*math.sin(a):.1f}')
b += f'<g filter="url(#sh)"><circle cx="{ix}" cy="{iy}" r="52" fill="{L1}"/></g>'
b += f'<circle cx="{ix}" cy="{iy}" r="{R}" fill="none" stroke="{L4}" stroke-width="1.5" stroke-dasharray="3 4"/>'
b += f'<polyline points="{" ".join(pts)}" fill="none" stroke="{BLUE}" stroke-width="3" stroke-linejoin="round"/>'
b += f'<circle cx="{ix}" cy="{iy}" r="7" fill="{RED}"/>'
# a small sun: energy comes from outside
b += f'<circle cx="70" cy="56" r="22" fill="{YEL}"/><circle cx="70" cy="56" r="32" fill="{YEL}" opacity=".25"/>'
imgs["atom-waves"] = b

# 9. Hypothesis 2: no void — zoom into a "point" and find smaller balls again
b = ''
# background: space filled with tiny dots everywhere (no emptiness)
import random
rnd = random.Random(7)
for _ in range(260):
    x, y = rnd.uniform(8, 552), rnd.uniform(8, 292)
    b += f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{rnd.uniform(0.8, 2.2):.1f}" fill="{BLUE5}" opacity="{rnd.uniform(.12, .3):.2f}"/>'
stages = [(80, 150, 62), (215, 150, 50), (330, 150, 40), (425, 150, 31), (500, 150, 24)]
cols = [BLUE, RED, ORANGE, PURP, GREEN]
for i in range(len(stages) - 1):
    (x0, y0, r0), (x1, y1, r1) = stages[i], stages[i + 1]
    # zoom cone from a small ball inside stage i to stage i+1
    a0 = -0.5
    sx, sy, sr = x0 + r0 * 0.42 * math.cos(a0), y0 + r0 * 0.42 * math.sin(a0), r0 * 0.16
    b += f'<path d="M{sx} {sy-sr} L{x1} {y1-r1} L{x1} {y1+r1} L{sx} {sy+sr} Z" fill="{cols[i+1]}" opacity=".10"/>'
    b += f'<line x1="{sx}" y1="{sy-sr}" x2="{x1}" y2="{y1-r1}" stroke="{INK2}" stroke-width="1.2" stroke-dasharray="3 4"/><line x1="{sx}" y1="{sy+sr}" x2="{x1}" y2="{y1+r1}" stroke="{INK2}" stroke-width="1.2" stroke-dasharray="3 4"/>'
for i, (x, y, r) in enumerate(stages):
    c = cols[i]
    b += f'<g filter="url(#sh)"><circle cx="{x}" cy="{y}" r="{r}" fill="{L1}" stroke="{c}" stroke-width="2.5"/></g>'
    # packed small balls inside (the "micro-balls")
    rr = r * 0.16
    for ring, n in [(0.0, 1), (0.42, 7), (0.75, 13)]:
        for k in range(n):
            a = 2 * math.pi * k / n - 0.5
            px, py = x + ring * r * math.cos(a), y + ring * r * math.sin(a)
            hl = (ring == 0.42 and k == 0) and i < len(stages) - 1
            b += f'<circle cx="{px:.1f}" cy="{py:.1f}" r="{rr:.1f}" fill="{c}" opacity="{1 if hl else 0.5}"' + (f' stroke="{INK}" stroke-width="1.4"' if hl else '') + '/>'
for k in range(3):
    b += f'<circle cx="{534 + k*9}" cy="150" r="2.6" fill="{INK2}" opacity="{0.8 - k*0.25:.2f}"/>'
# a magnifying glass over the first stage
b += f'<circle cx="80" cy="150" r="72" fill="none" stroke="{INK2}" stroke-width="5" opacity=".75"/><line x1="131" y1="201" x2="160" y2="232" stroke="{INK2}" stroke-width="10" stroke-linecap="round" opacity=".75"/>'
imgs["atom-fractal"] = b

for name, body in imgs.items():
    p = os.path.join(OUT, name + ".svg")
    open(p, "w").write(svg(body))
    hgt = 260 if name == "structural-differential" else 300
    if hgt != 300:
        open(p, "w").write(svg(body, h=hgt))
    k = 2 if name in ("atom-waves", "atom-fractal") else 1  # newer pictures: sharp on HiDPI
    subprocess.run(["rsvg-convert", "-w", str(560 * k), "-h", str(hgt * k), "-o", os.path.join(AS, name + ".png"), p], check=True)
print("ok", len(imgs))
