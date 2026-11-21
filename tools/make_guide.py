#!/usr/bin/env python3
"""Wordless step-by-step illustrations in the GNOME/Adwaita style (SVG + PNG)."""
import math, os, subprocess
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(root, "docs", "img"); AS = os.path.join(root, "assets", "guide")
os.makedirs(OUT, exist_ok=True); os.makedirs(AS, exist_ok=True)

BLUE, BLUE1, BLUE5 = "#3584e4", "#99c1f1", "#1a5fb4"
GREEN, RED, YEL, PURP, ORANGE = "#2ec27e", "#e01b24", "#f6d32d", "#9141ac", "#ff7800"
INK, INK2, L1, L2, L3, L4 = "#241f31", "#77767b", "#ffffff", "#f6f5f4", "#deddda", "#c0bfbc"

def svg(body, w=480, h=300):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
<defs>
<filter id="sh" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#000" flood-opacity=".13"/></filter>
<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#eaf2fd"/><stop offset="1" stop-color="#f6f5f4"/></linearGradient>
<linearGradient id="vid" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#99c1f1"/><stop offset="1" stop-color="#1c71d8"/></linearGradient>
<clipPath id="circ"><circle cx="240" cy="150" r="92"/></clipPath>
</defs>
<rect width="{w}" height="{h}" rx="20" fill="url(#sky)"/>
<circle cx="70" cy="250" r="90" fill="{BLUE1}" opacity=".18"/><circle cx="430" cy="40" r="70" fill="{BLUE1}" opacity=".16"/>
{body}</svg>'''

def window(x, y, w, h, body="", title_dots=True):
    s = f'<g filter="url(#sh)"><rect x="{x}" y="{y}" width="{w}" height="{h}" rx="12" fill="{L1}" stroke="{L3}"/></g>'
    s += f'<path d="M{x} {y+12} a12 12 0 0 1 12-12 h{w-24} a12 12 0 0 1 12 12 v22 h-{w} z" fill="{L2}"/>'
    s += f'<line x1="{x}" y1="{y+34}" x2="{x+w}" y2="{y+34}" stroke="{L3}"/>'
    if title_dots:
        s += f'<circle cx="{x+w-18}" cy="{y+17}" r="6" fill="{L3}"/><path d="M{x+w-20.5} {y+14.5} l5 5 m0 -5 l-5 5" stroke="{INK2}" stroke-width="1.5" stroke-linecap="round"/>'
        s += f'<rect x="{x+w/2-34}" y="{y+13}" width="68" height="8" rx="4" fill="{L4}"/>'
    return s + body

def bar(x, y, w, c=L3, h=8):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{h/2}" fill="{c}"/>'

def avatar(cx, cy, r, c, ring=None):
    s = ""
    if ring: s += f'<circle cx="{cx}" cy="{cy}" r="{r+4}" fill="none" stroke="{ring}" stroke-width="3"/>'
    s += f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{c}"/>'
    s += f'<circle cx="{cx}" cy="{cy-r*0.22}" r="{r*0.36}" fill="#fff" opacity=".95"/>'
    s += f'<path d="M{cx-r*0.62} {cy+r*0.62} a{r*0.62} {r*0.5} 0 0 1 {r*1.24} 0" fill="#fff" opacity=".95"/>'
    return s

def laptop(cx, cy, s=1.0, screen=BLUE, inner=""):
    w, h = 64*s, 42*s
    return (f'<g filter="url(#sh)"><rect x="{cx-w/2}" y="{cy-h}" width="{w}" height="{h}" rx="{5*s}" fill="{INK}"/></g>'
            f'<rect x="{cx-w/2+4*s}" y="{cy-h+4*s}" width="{w-8*s}" height="{h-8*s}" rx="{2*s}" fill="{screen}"/>{inner}'
            f'<path d="M{cx-w/2-9*s} {cy+1} h{w+18*s} l-{5*s} {7*s} h-{w+8*s} z" fill="{L4}"/>')

def smiley(cx, cy, r):
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{YEL}"/><circle cx="{cx-r*.35}" cy="{cy-r*.2}" r="{r*.12}" fill="#613583"/>'
            f'<circle cx="{cx+r*.35}" cy="{cy-r*.2}" r="{r*.12}" fill="#613583"/>'
            f'<path d="M{cx-r*.5} {cy+r*.18} q{r*.5} {r*.55} {r} 0" fill="none" stroke="#613583" stroke-width="{r*.12}" stroke-linecap="round"/>')

def speaker(x, y, c=INK2, s=1.0):
    return (f'<path d="M{x} {y-4*s} h{5*s} l{6*s} -{5*s} v{18*s} l-{6*s} -{5*s} h-{5*s} z" fill="{c}"/>'
            f'<path d="M{x+14*s} {y-2*s} a{5*s} {5*s} 0 0 1 0 {8*s}" fill="none" stroke="{c}" stroke-width="{2*s}" stroke-linecap="round"/>')

def mic(cx, cy, c="#fff", s=1.0):
    return (f'<rect x="{cx-4*s}" y="{cy-9*s}" width="{8*s}" height="{13*s}" rx="{4*s}" fill="{c}"/>'
            f'<path d="M{cx-7.5*s} {cy} a{7.5*s} {7.5*s} 0 0 0 {15*s} 0 M{cx} {cy+7.5*s} v{4*s}" fill="none" stroke="{c}" stroke-width="{2*s}" stroke-linecap="round"/>')

imgs = {}

# 1 — the app scans for nodes; no central server
nodes = [(80, 80), (400, 78), (418, 214), (70, 220), (240, 266)]
b = "".join(f'<circle cx="240" cy="150" r="{r}" fill="none" stroke="{BLUE}" stroke-width="2" opacity="{o}"/>' for r, o in ((62, .55), (96, .35), (132, .18)))
b += "".join(f'<line x1="240" y1="150" x2="{x}" y2="{y}" stroke="{BLUE}" stroke-width="2.5" stroke-dasharray="2 7" stroke-linecap="round"/>' for x, y in nodes)
b += f'<line x1="80" y1="80" x2="70" y2="220" stroke="{L4}" stroke-width="2" stroke-dasharray="2 7" stroke-linecap="round"/><line x1="400" y1="78" x2="418" y2="214" stroke="{L4}" stroke-width="2" stroke-dasharray="2 7" stroke-linecap="round"/>'
b += laptop(240, 170, 1.25, BLUE, f'<circle cx="240" cy="144" r="10" fill="none" stroke="#fff" stroke-width="2.5"/><circle cx="240" cy="144" r="3" fill="#fff"/>')
for (x, y), c in zip(nodes, (GREEN, PURP, ORANGE, GREEN, BLUE5)):
    b += avatar(x, y, 17, c)
# crossed-out server rack
b += f'<g transform="translate(300,6)" opacity=".9"><rect x="0" y="0" width="40" height="52" rx="6" fill="{L1}" stroke="{L4}" stroke-width="2"/>' + "".join(f'<rect x="7" y="{8+i*14}" width="26" height="8" rx="3" fill="{L3}"/><circle cx="29" cy="{12+i*14}" r="2" fill="{INK2}"/>' for i in range(3)) + f'<circle cx="34" cy="46" r="13" fill="{RED}"/><path d="M28 40 l12 12 m0 -12 l-12 12" stroke="#fff" stroke-width="3" stroke-linecap="round"/></g>'
imgs["guide-1-network"] = b

# 2 — copy your connect code and send it
b = window(40, 40, 270, 220)
b += bar(60, 92, 120, INK, 10) + bar(60, 112, 200)
b += f'<rect x="60" y="132" width="228" height="62" rx="8" fill="{L2}" stroke="{L3}"/>' + bar(72, 146, 190, L4, 7) + bar(72, 160, 205, L4, 7) + bar(72, 174, 150, L4, 7)
b += f'<rect x="60" y="208" width="96" height="30" rx="8" fill="{BLUE}"/><rect x="74" y="218" width="12" height="12" rx="2" fill="none" stroke="#fff" stroke-width="2"/><rect x="78" y="214" width="12" height="12" rx="2" fill="{BLUE}" stroke="#fff" stroke-width="2"/>' + bar(98, 220, 46, "#fff", 7)
b += f'<path d="M130 236 l0 22 l6 -6 l5 11 l5 -2 l-5 -11 l8 0 z" fill="{INK}" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/>'
b += f'<path d="M318 150 C345 120 360 120 378 132" fill="none" stroke="{BLUE}" stroke-width="3" stroke-dasharray="3 8" stroke-linecap="round"/>'
b += f'<g transform="translate(372,98) rotate(-12)" filter="url(#sh)"><path d="M0 30 L62 0 L46 58 L30 40 Z" fill="{L1}" stroke="{BLUE}" stroke-width="3" stroke-linejoin="round"/><path d="M30 40 L62 0 M30 40 L28 56 L38 46" fill="none" stroke="{BLUE}" stroke-width="3" stroke-linejoin="round"/></g>'
b += avatar(410, 222, 26, PURP)
imgs["guide-2-code"] = b

# 3 — add your friend
b = window(150, 34, 180, 92)
b += f'<rect x="166" y="80" width="148" height="30" rx="8" fill="{L2}" stroke="{BLUE}" stroke-width="2"/>' + bar(176, 91, 100, L4, 8)
b += f'<circle cx="300" cy="95" r="9" fill="{BLUE}"/><path d="M300 90 v10 m-5 -5 h10" stroke="#fff" stroke-width="2.4" stroke-linecap="round"/>'
b += f'<path d="M150 215 C200 165 280 165 330 215" fill="none" stroke="{GREEN}" stroke-width="5" stroke-linecap="round"/>'
b += avatar(120, 220, 40, BLUE) + avatar(360, 220, 40, PURP)
b += f'<circle cx="150" cy="250" r="10" fill="{GREEN}" stroke="#fff" stroke-width="4"/><circle cx="390" cy="250" r="10" fill="{GREEN}" stroke="#fff" stroke-width="4"/>'
b += f'<g transform="translate(240,178)"><circle r="20" fill="{L1}" stroke="{GREEN}" stroke-width="3" filter="url(#sh)"/><path d="M-8 0 l5 6 l11 -12" fill="none" stroke="{GREEN}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></g>'
imgs["guide-3-friend"] = b

# 4 — chat with emoji and GIFs
b = window(60, 22, 360, 258)
b += avatar(88, 78, 13, PURP) + f'<rect x="108" y="64" width="170" height="30" rx="14" fill="{L2}"/>' + bar(122, 75, 120, L4)
b += avatar(88, 124, 13, PURP) + smiley(132, 124, 20)
b += f'<rect x="200" y="106" width="190" height="30" rx="14" fill="{BLUE}"/>' + bar(214, 117, 140, "#ffffff", 8)
# GIF tile
b += avatar(88, 176, 13, BLUE) + f'<rect x="108" y="154" width="120" height="78" rx="10" fill="url(#vid)"/>'
b += f'<circle cx="150" cy="190" r="16" fill="{YEL}"/><path d="M120 232 l30 -24 l22 16 l18 -12 l38 20 z" fill="#1a5fb4" opacity=".7"/>'
b += f'<rect x="114" y="160" width="30" height="16" rx="4" fill="#000" opacity=".45"/><text x="129" y="172" font-family="Inter, Cantarell, sans-serif" font-size="10" font-weight="700" fill="#fff" text-anchor="middle">GIF</text>'
# composer
b += f'<line x1="60" y1="244" x2="420" y2="244" stroke="{L3}"/>' + smiley(84, 262, 9)
b += f'<rect x="100" y="253" width="30" height="18" rx="5" fill="none" stroke="{BLUE}" stroke-width="2"/><text x="115" y="266" font-family="Inter, Cantarell, sans-serif" font-size="9" font-weight="700" fill="{BLUE}" text-anchor="middle">GIF</text>'
b += f'<circle cx="148" cy="262" r="8" fill="none" stroke="{INK2}" stroke-width="2"/><circle cx="148" cy="262" r="3.5" fill="{RED}"/>'
b += f'<rect x="166" y="252" width="170" height="20" rx="6" fill="{L2}" stroke="{L3}"/><rect x="344" y="252" width="62" height="20" rx="6" fill="{BLUE}"/>' + bar(358, 258, 34, "#fff", 8)
imgs["guide-4-chat"] = b

# 5 — record a video circle
b = f'<circle cx="240" cy="150" r="104" fill="{L1}" filter="url(#sh)"/>'
b += f'<g clip-path="url(#circ)"><rect x="140" y="50" width="200" height="200" fill="url(#vid)"/>'
b += f'<circle cx="240" cy="132" r="34" fill="#f9d2b0"/><path d="M206 120 a34 34 0 0 1 68 0 a40 30 0 0 0 -68 0z" fill="#63452c"/><path d="M168 250 a72 64 0 0 1 144 0z" fill="#ffffff" opacity=".9"/>'
b += f'<circle cx="228" cy="132" r="3" fill="{INK}"/><circle cx="252" cy="132" r="3" fill="{INK}"/><path d="M230 148 q10 8 20 0" fill="none" stroke="{INK}" stroke-width="2.5" stroke-linecap="round"/></g>'
ang = 2 * math.pi * 0.68
ex, ey = 240 + 98 * math.sin(ang), 150 - 98 * math.cos(ang)
b += f'<circle cx="240" cy="150" r="98" fill="none" stroke="{L3}" stroke-width="6"/><path d="M240 52 A98 98 0 1 1 {ex:.1f} {ey:.1f}" fill="none" stroke="{RED}" stroke-width="6" stroke-linecap="round"/>'
b += f'<rect x="210" y="226" width="60" height="22" rx="11" fill="#000" opacity=".5"/><circle cx="224" cy="237" r="5" fill="{RED}"/>' + bar(234, 233, 26, "#fff", 8)
# webcam
b += f'<g transform="translate(64,62)" filter="url(#sh)"><rect x="0" y="0" width="70" height="40" rx="20" fill="{INK}"/><circle cx="35" cy="20" r="13" fill="#3d3846"/><circle cx="35" cy="20" r="7" fill="{BLUE5}"/><circle cx="32" cy="17" r="2.5" fill="#fff" opacity=".8"/><circle cx="60" cy="10" r="3" fill="{RED}"/><path d="M25 40 l-6 24 h32 l-6 -24" fill="{L4}"/></g>'
b += f'<g transform="translate(372,196)"><circle r="30" fill="{BLUE}" filter="url(#sh)"/><path d="M-12 2 L-2 -12 M-12 2 L12 2 M-2 16 L12 2 M-2 -12 L12 2" stroke="none"/><path d="M-11 0 h20 m-8 -9 l9 9 l-9 9" fill="none" stroke="#fff" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/></g>'
imgs["guide-5-circle"] = b

# 6 — voice channels
b = window(60, 22, 240, 258, title_dots=False)
b += bar(78, 52, 90, INK, 9)
b += f'<text x="78" y="82" font-family="Inter, Cantarell, sans-serif" font-size="14" font-weight="700" fill="{INK2}">#</text>' + bar(94, 74, 90) + f'<text x="78" y="102" font-family="Inter, Cantarell, sans-serif" font-size="14" font-weight="700" fill="{INK2}">#</text>' + bar(94, 94, 70)
b += f'<line x1="74" y1="114" x2="286" y2="114" stroke="{L3}"/>'
b += f'<rect x="70" y="122" width="220" height="28" rx="8" fill="{BLUE1}" opacity=".45"/>' + speaker(80, 136, BLUE5) + bar(104, 132, 96, BLUE5)
b += avatar(104, 168, 11, GREEN, ring=GREEN) + bar(124, 164, 60) + avatar(104, 196, 11, PURP) + bar(124, 192, 74) + avatar(104, 224, 11, ORANGE) + bar(124, 220, 50)
b += speaker(80, 252, INK2) + bar(104, 248, 80)
# voice connected bar on the right side
b += f'<g filter="url(#sh)"><rect x="318" y="150" width="130" height="96" rx="14" fill="{L1}" stroke="{L3}"/></g>'
b += f'<circle cx="336" cy="170" r="5" fill="{GREEN}"/>' + bar(348, 166, 80, GREEN)
b += f'<circle cx="352" cy="214" r="20" fill="{GREEN}"/>' + mic(352, 214, "#fff", 1.05)
b += f'<circle cx="412" cy="214" r="20" fill="{L3}"/><path d="M398 216c0-4 6-8 14-8s14 4 14 8v3c0 1-1 1.5-2 1.4l-4-1c-1-.2-1.5-1-1.5-1.8v-2c-4-1-9-1-13 0v2c0 .8-.5 1.6-1.5 1.8l-4 1c-1 .1-2-.4-2-1.4z" fill="{RED}"/>'
# sound waves from the talking person
b += "".join(f'<path d="M{358-i*13} {92-(14+i*9)} a{26+i*12} {26+i*12} 0 0 0 0 {2*(14+i*9)}" fill="none" stroke="{GREEN}" stroke-width="3" stroke-linecap="round" opacity="{.9-i*.25}"/>' for i in range(3))
b += avatar(400, 92, 30, GREEN, ring=GREEN)
imgs["guide-6-voice"] = b

for name, body in imgs.items():
    p = os.path.join(OUT, name + ".svg")
    open(p, "w").write(svg(body))
    subprocess.run(["rsvg-convert", "-w", "480", "-h", "300", "-o", os.path.join(AS, name + ".png"), p], check=True)
print("ok", len(imgs))
