#!/usr/bin/env python3
"""Generate GNOME-style server tiles and symbolic icons (SVG), then PNG via rsvg-convert."""
import math, os, subprocess, sys
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SV = os.path.join(root, "assets", "servers"); SY = os.path.join(root, "assets", "symbolic")

# GNOME palette shades (2, 4, 5) per tile
PAL = {
 "blue":   ("#62a0ea", "#1c71d8", "#1a5fb4"),
 "purple": ("#c061cb", "#813d9c", "#613583"),
 "green":  ("#57e389", "#2ec27e", "#26a269"),
 "teal":   ("#5bc8af", "#2190a4", "#1c6f80"),
 "orange": ("#ffa348", "#e66100", "#c64600"),
 "yellow": ("#f8e45c", "#f5c211", "#e5a50a"),
 "red":    ("#ed333b", "#c01c28", "#a51d2d"),
 "brown":  ("#b5835a", "#865e3c", "#63452c"),
 "slate":  ("#8b9eb0", "#5d7385", "#475968"),
}
W = 'fill="none" stroke="#fff" stroke-width="3.4" stroke-linecap="round" stroke-linejoin="round"'

def tile(color, glyph):
    a, b, edge = PAL[color]
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient>
<linearGradient id="hl" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".28"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/></linearGradient></defs>
<rect x="4" y="6" width="56" height="54" rx="13" fill="{edge}"/>
<rect x="4" y="4" width="56" height="52" rx="13" fill="url(#g)"/>
<rect x="4" y="4" width="56" height="52" rx="13" fill="url(#hl)"/>
<g transform="translate(0,-1)">{glyph}</g>
</svg>'''

def rays(cx, cy, r1, r2, n, off=0):
    out = []
    for i in range(n):
        a = 2 * math.pi * i / n + off
        out.append(f'<line x1="{cx+r1*math.cos(a):.2f}" y1="{cy+r1*math.sin(a):.2f}" x2="{cx+r2*math.cos(a):.2f}" y2="{cy+r2*math.sin(a):.2f}"/>')
    return "".join(out)

C = (32, 31)
glyphs = {
 # Venus Project style circular city: rings + radial avenues
 "sce": ("blue", f'<g {W}><circle cx="32" cy="31" r="16"/><circle cx="32" cy="31" r="8.5"/>{rays(32,31,8.5,16,8,math.pi/8)}</g><circle cx="32" cy="31" r="3.6" fill="#fff"/>'),
 # Atom: three orbits + nucleus
 "atom-generator": ("purple", '<g fill="none" stroke="#fff" stroke-width="2.8">' + "".join(f'<ellipse cx="32" cy="31" rx="18" ry="6.6" transform="rotate({a} 32 31)"/>' for a in (0, 60, 120)) + '</g><circle cx="32" cy="31" r="4" fill="#fff"/>'),
 # Chip with pins
 "automation": ("green", f'<g {W}><rect x="21" y="20" width="22" height="22" rx="3.5"/>' + "".join(f'<line x1="{x}" y1="14" x2="{x}" y2="19"/><line x1="{x}" y1="43" x2="{x}" y2="48"/>' for x in (26, 32, 38)) + "".join(f'<line x1="15" y1="{y}" x2="20" y2="{y}"/><line x1="44" y1="{y}" x2="49" y2="{y}"/>' for y in (25, 31, 37)) + '</g><rect x="27" y="26" width="10" height="10" rx="1.5" fill="#fff"/>'),
 # City skyline + sparkle (AI design / architecture)
 "ai-design": ("teal", f'<g {W}><path d="M14 45 H50"/><path d="M18 45 V30 H26 V45"/><path d="M26 45 V22 H35 V45"/><path d="M35 45 V34 H44 V45"/></g><path d="M45 13 l1.8 4.6 4.6 1.8 -4.6 1.8 -1.8 4.6 -1.8 -4.6 -4.6 -1.8 4.6 -1.8z" fill="#fff"/><g fill="#fff"><rect x="29" y="27" width="3" height="3"/><rect x="29" y="33" width="3" height="3"/><rect x="21" y="34" width="3" height="3"/></g>'),
 # Painter's palette
 "art": ("orange", '<path d="M32 14c-10 0-18 7.6-18 17s7 16 15 16c3 0 4-2 3-4.2-1-2.4.6-4.8 3.4-4.8H40c5.6 0 10-3.8 10-9C50 20.6 42 14 32 14z" fill="#fff"/><circle cx="23.5" cy="29" r="3" fill="#3584e4"/><circle cx="28.5" cy="21.5" r="3" fill="#e01b24"/><circle cx="37" cy="21" r="3" fill="#f5c211"/><circle cx="43" cy="27.5" r="3" fill="#2ec27e"/>'),
 # Light bulb with rays
 "creativity": ("yellow", f'<g {W}><path d="M26.5 38 C26.5 33 21 31 21 24.5 a11 11 0 0 1 22 0 C43 31 37.5 33 37.5 38z"/><line x1="27" y1="43" x2="37" y2="43"/><line x1="29" y1="47.5" x2="35" y2="47.5"/></g><g stroke="#fff" stroke-width="2.6" stroke-linecap="round">{rays(32,24.5,15,18.5,5,-math.pi/2-math.pi/2)}</g>'),
 # Folded map + pin: the map is not the territory
 "general-semantics": ("red", f'<g {W}><path d="M14 20 L24 16 L40 20 L50 16 V42 L40 46 L24 42 L14 46 Z"/><path d="M24 16 V42 M40 20 V46" stroke-width="2.4"/></g><path d="M32 22a6 6 0 0 1 6 6c0 4.5-6 10-6 10s-6-5.5-6-10a6 6 0 0 1 6-6z" fill="#fff"/><circle cx="32" cy="28" r="2.2" fill="#c01c28"/>'),
 # Archive box
 "archives": ("brown", f'<g {W}><rect x="15" y="16" width="34" height="9" rx="2.5"/><path d="M18 25 V44 a3 3 0 0 0 3 3 H43 a3 3 0 0 0 3-3 V25"/><line x1="27" y1="33" x2="37" y2="33"/></g>'),
 # Lifebuoy
 "help": ("slate", '<circle cx="32" cy="31" r="15.5" fill="#fff"/><circle cx="32" cy="31" r="7.5" fill="#5d7385"/>' + "".join(f'<path d="M{32+7.5*math.cos(a-0.42):.2f} {31+7.5*math.sin(a-0.42):.2f} L{32+15.5*math.cos(a-0.3):.2f} {31+15.5*math.sin(a-0.3):.2f} A15.5 15.5 0 0 1 {32+15.5*math.cos(a+0.3):.2f} {31+15.5*math.sin(a+0.3):.2f} L{32+7.5*math.cos(a+0.42):.2f} {31+7.5*math.sin(a+0.42):.2f} A7.5 7.5 0 0 0 {32+7.5*math.cos(a-0.42):.2f} {31+7.5*math.sin(a-0.42):.2f}z" fill="#e01b24"/>' for a in (math.pi/4, 3*math.pi/4, 5*math.pi/4, 7*math.pi/4))),
 # Network map: nodes linked in a mesh, no centre server
 "network": ("teal", '<g stroke="#fff" stroke-width="2.4" stroke-linecap="round" opacity=".9"><line x1="32" y1="31" x2="17" y2="19"/><line x1="32" y1="31" x2="47" y2="18"/><line x1="32" y1="31" x2="18" y2="44"/><line x1="32" y1="31" x2="46" y2="44"/><line x1="17" y1="19" x2="47" y2="18" stroke-dasharray="2 4"/><line x1="18" y1="44" x2="46" y2="44" stroke-dasharray="2 4"/></g><circle cx="32" cy="31" r="6.5" fill="#fff"/><circle cx="17" cy="19" r="4.5" fill="#fff"/><circle cx="47" cy="18" r="4.5" fill="#fff"/><circle cx="18" cy="44" r="4.5" fill="#fff"/><circle cx="46" cy="44" r="4.5" fill="#fff"/>'),
 # Friends tile (for docs)
 "friends": ("blue", f'<g {W}><circle cx="26" cy="24" r="6"/><path d="M15 44c0-7 5-11 11-11s11 4 11 11"/><circle cx="40" cy="22" r="5" stroke-width="2.8"/><path d="M39 31c6 0 10 3.5 10 10" stroke-width="2.8"/></g>'),
}

# Symbolic icons (16px grid scaled x2), white on transparent — tinted at runtime.
S = 'fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"'
def sym(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">{body}</svg>'
symbolic = {
 "speaker": f'<path d="M5 12 H10 L17 6 V26 L10 20 H5 Z" fill="#fff"/><g {S}><path d="M21 11.5a6 6 0 0 1 0 9"/><path d="M24.5 8a11 11 0 0 1 0 16"/></g>',
 "hash": f'<g {S}><line x1="12" y1="5" x2="10" y2="27"/><line x1="22" y1="5" x2="20" y2="27"/><line x1="6" y1="12" x2="27" y2="12"/><line x1="5" y1="20" x2="26" y2="20"/></g>',
 "mic": f'<rect x="11" y="3.5" width="10" height="16" rx="5" fill="#fff"/><g {S}><path d="M7 15a9 9 0 0 0 18 0"/><line x1="16" y1="24" x2="16" y2="28.5"/></g>',
 "mic-off": f'<rect x="11" y="3.5" width="10" height="16" rx="5" fill="#fff" opacity=".55"/><g {S}><path d="M7 15a9 9 0 0 0 18 0" opacity=".55"/><line x1="16" y1="24" x2="16" y2="28.5" opacity=".55"/><line x1="5" y1="4" x2="27" y2="28" stroke-width="3"/></g>',
 "hangup": '<path d="M3 18.5c0-2.5 6-6.5 13-6.5s13 4 13 6.5v3c0 .8-.7 1.4-1.5 1.3l-5-1c-.7-.1-1.2-.8-1.2-1.5v-2.6c-3.6-1-7-1-10.6 0v2.6c0 .7-.5 1.4-1.2 1.5l-5 1C3.7 22.9 3 22.3 3 21.5z" fill="#fff"/>',
 "gear": '<path fill="#fff" fill-rule="evenodd" d="' + " ".join(
     (lambda pts: "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in pts) + "Z")(
         [(16 + (12 if (i % 4) in (0, 1) else 9.2) * math.cos(2*math.pi*i/32 + math.pi/32), 16 + (12 if (i % 4) in (0, 1) else 9.2) * math.sin(2*math.pi*i/32 + math.pi/32)) for i in range(32)]
     ).split()) + ' M16 11.5a4.5 4.5 0 1 0 0.01 0Z"/>',
 "people": f'<g {S}><circle cx="12" cy="11" r="4.5"/><path d="M4 26c0-5 3.6-8 8-8s8 3 8 8"/><circle cx="22.5" cy="10" r="3.5" stroke-width="2.2"/><path d="M22 17c4 0 7 2.5 7 7" stroke-width="2.2"/></g>',
 "home": f'<g {S}><path d="M5 15 L16 5.5 L27 15"/><path d="M8.5 13 V26.5 H23.5 V13"/></g><rect x="13.5" y="18" width="5" height="8.5" rx="1" fill="#fff"/>',
 "pencil": f'<g {S}><path d="M20.5 6.5 L25.5 11.5 L12 25 H7 V20 Z"/><line x1="17.5" y1="9.5" x2="22.5" y2="14.5"/></g>',
 "trash": f'<g {S}><line x1="5.5" y1="9" x2="26.5" y2="9"/><path d="M12.5 9 V6 H19.5 V9"/><path d="M8.5 9 L10 27 H22 L23.5 9"/><line x1="13.5" y1="14" x2="14" y2="22.5"/><line x1="18.5" y1="14" x2="18" y2="22.5"/></g>',
 "levels": f'<g {S}><path d="M5 9 Q16 -1 27 9"/><line x1="16" y1="8" x2="16" y2="12"/><circle cx="16" cy="15" r="3"/><line x1="16" y1="18" x2="16" y2="21"/></g><rect x="10" y="21" width="12" height="5" rx="1.5" fill="#fff"/><rect x="12" y="27.5" width="8" height="3" rx="1" fill="#fff" opacity=".7"/>',
 "image": f'<g {S}><rect x="4.5" y="6.5" width="23" height="19" rx="3"/><path d="M5 22 L12 15 L17 20 L20.5 16.5 L27 23"/></g><circle cx="21" cy="12" r="2.4" fill="#fff"/>',
 "sigma": f'<g {S}><path d="M24 7 H8 L16 16 L8 25 H24"/></g>',
 "headphones": f'<g {S}><path d="M6 20v-4a10 10 0 0 1 20 0v4"/></g><rect x="4.5" y="18" width="6" height="9" rx="2.5" fill="#fff"/><rect x="21.5" y="18" width="6" height="9" rx="2.5" fill="#fff"/>',
}

def render(svg_path, png_path, size):
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", png_path, svg_path], check=True)

for slug, (col, g) in glyphs.items():
    p = os.path.join(SV, slug + ".svg")
    open(p, "w").write(tile(col, g))
    render(p, os.path.join(SV, slug + ".png"), 128)
for name, body in symbolic.items():
    p = os.path.join(SY, name + ".svg")
    open(p, "w").write(sym(body))
    render(p, os.path.join(SY, name + ".png"), 64)
print("ok", len(glyphs), "tiles,", len(symbolic), "symbolic")
