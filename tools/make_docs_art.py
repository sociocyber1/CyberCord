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

for name, body in imgs.items():
    p = os.path.join(OUT, name + ".svg")
    open(p, "w").write(svg(body))
    hgt = 260 if name == "structural-differential" else 300
    if hgt != 300:
        open(p, "w").write(svg(body, h=hgt))
    subprocess.run(["rsvg-convert", "-w", "560", "-h", str(hgt), "-o", os.path.join(AS, name + ".png"), p], check=True)
print("ok", len(imgs))
