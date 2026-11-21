#!/usr/bin/env python3
"""SocioCyberneering / general-semantics emoji set (GNOME style), SVG → 72 px PNG."""
import math, os, subprocess
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(root, "assets", "emoji", "sce"); os.makedirs(OUT, exist_ok=True)
F = "Inter, Cantarell, sans-serif"
B, B1, B5, G, G5, O, P, R, Y, BR = "#3584e4", "#99c1f1", "#1a5fb4", "#2ec27e", "#26a269", "#ff7800", "#9141ac", "#e01b24", "#f6d32d", "#865e3c"
INK, L1, L3, L4 = "#241f31", "#ffffff", "#deddda", "#c0bfbc"

def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="72" height="72" viewBox="0 0 72 72">{body}</svg>'

def person(x, y, s, c):
    return f'<circle cx="{x}" cy="{y-9*s}" r="{6*s}" fill="{c}"/><path d="M{x-10*s} {y+12*s} v-4 a{10*s} {9*s} 0 0 1 {20*s} 0 v4 z" fill="{c}"/>'

E = {}
# map ≠ territory
E["gs_map"] = (f'<path d="M6 16 L22 10 L38 16 L54 10 V50 L38 56 L22 50 L6 56 Z" fill="#f9f06b" stroke="{BR}" stroke-width="2.5" stroke-linejoin="round"/><path d="M22 10 V50 M38 16 V56" stroke="{BR}" stroke-width="2"/>'
               f'<circle cx="56" cy="54" r="14" fill="{R}"/><path d="M49 50 h14 M49 57 h14" stroke="#fff" stroke-width="3" stroke-linecap="round"/><path d="M53 63 L59 45" stroke="#fff" stroke-width="3" stroke-linecap="round"/>', "map is not the territory")
# indexing: two people ₁ ₂
E["gs_index"] = (person(22, 34, 1.3, B) + person(50, 34, 1.3, P) + f'<text x="22" y="66" font-family="{F}" font-size="16" font-weight="800" fill="{B5}" text-anchor="middle">₁</text><text x="50" y="66" font-family="{F}" font-size="16" font-weight="800" fill="{P}" text-anchor="middle">₂</text><text x="36" y="40" font-family="{F}" font-size="18" font-weight="800" fill="{INK}" text-anchor="middle">≠</text>', "indexing")
# dating: calendar
E["gs_date"] = (f'<rect x="8" y="12" width="56" height="52" rx="10" fill="{L1}" stroke="{L4}" stroke-width="2"/><path d="M8 22 a10 10 0 0 1 10 -10 h36 a10 10 0 0 1 10 10 v8 h-56 z" fill="{R}"/><rect x="20" y="6" width="6" height="14" rx="3" fill="{INK}"/><rect x="46" y="6" width="6" height="14" rx="3" fill="{INK}"/>'
                f'<text x="36" y="56" font-family="{F}" font-size="22" font-weight="800" fill="{INK}" text-anchor="middle">²⁰²⁶</text>', "dating")
# etc.
E["gs_etc"] = (f'<path d="M10 16 h52 a6 6 0 0 1 6 6 v26 a6 6 0 0 1 -6 6 h-30 l-12 10 v-10 h-10 a6 6 0 0 1 -6 -6 v-26 a6 6 0 0 1 6 -6 z" fill="{B}"/>' + ''.join(f'<circle cx="{22+i*14}" cy="35" r="5" fill="#fff"/>' for i in range(3)), "et cetera")
# safety quotes
E["gs_quotes"] = (f'<rect x="6" y="14" width="60" height="44" rx="12" fill="{G5}"/><text x="36" y="49" font-family="{F}" font-size="36" font-weight="800" fill="#fff" text-anchor="middle">«»</text>', "safety quotes")
# hyphen: space-time
E["gs_hyphen"] = (f'<rect x="4" y="22" width="24" height="28" rx="6" fill="{B}"/><rect x="44" y="22" width="24" height="28" rx="6" fill="{P}"/><rect x="28" y="32" width="16" height="8" rx="4" fill="{INK}"/>', "hyphen")
# delayed reaction: hourglass
E["gs_pause"] = (f'<rect x="14" y="6" width="44" height="7" rx="3.5" fill="{BR}"/><rect x="14" y="59" width="44" height="7" rx="3.5" fill="{BR}"/><path d="M19 13 h34 c0 14 -12 18 -12 23 c0 5 12 9 12 23 h-34 c0 -14 12 -18 12 -23 c0 -5 -12 -9 -12 -23 z" fill="{B1}" stroke="{B5}" stroke-width="2"/><path d="M26 22 h20 c-2 6 -8 9 -10 14 c-2 -5 -8 -8 -10 -14 z" fill="{Y}"/><path d="M24 57 c2 -8 10 -10 12 -14 c2 4 10 6 12 14 z" fill="{Y}"/>', "delayed reaction")
# check the facts: magnifier + check
E["gs_check"] = (f'<circle cx="30" cy="30" r="20" fill="{L1}" stroke="{B5}" stroke-width="6"/><path d="M45 45 L62 62" stroke="{B5}" stroke-width="9" stroke-linecap="round"/><path d="M20 30 l7 7 l13 -14" fill="none" stroke="{G5}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>', "check the facts")
# hypothesis: flask with ?
E["gs_hypothesis"] = (f'<path d="M27 6 h18 v6 h-3 v14 l16 26 a7 7 0 0 1 -6 11 h-32 a7 7 0 0 1 -6 -11 l16 -26 v-14 h-3 z" fill="{L1}" stroke="{P}" stroke-width="3" stroke-linejoin="round"/><path d="M19 46 h34 l5 8 a4 4 0 0 1 -3 6 h-38 a4 4 0 0 1 -3 -6 z" fill="{P}" opacity=".85"/><text x="36" y="42" font-family="{F}" font-size="18" font-weight="800" fill="{P}" text-anchor="middle">?</text>', "test the hypothesis")
# structural differential
E["gs_sd"] = (f'<path d="M8 8 Q36 52 64 8" fill="#dcebfb" stroke="{B}" stroke-width="3"/><circle cx="26" cy="14" r="2" fill="{B}"/><circle cx="38" cy="18" r="2" fill="{B}"/><circle cx="46" cy="12" r="2" fill="{B}"/><ellipse cx="36" cy="42" rx="14" ry="5" fill="{L3}" stroke="{INK}" stroke-width="2"/><line x1="36" y1="30" x2="36" y2="38" stroke="{INK}" stroke-width="1.5"/><rect x="24" y="52" width="24" height="6" rx="2" fill="{G5}"/><rect x="27" y="61" width="18" height="6" rx="2" fill="{O}"/><line x1="36" y1="47" x2="36" y2="52" stroke="{INK}" stroke-width="1.5"/>', "structural differential")
# time-binding: torch passed along generations
E["gs_timebinding"] = (person(14, 44, 0.9, BR) + person(36, 44, 0.9, B) + person(58, 44, 0.9, G5) + f'<path d="M14 18 Q25 6 36 18 Q47 6 58 18" fill="none" stroke="{O}" stroke-width="3" stroke-dasharray="3 4" stroke-linecap="round"/><path d="M58 8 l3 6 l6 1 l-5 4 l1 6 l-5 -3 l-5 3 l1 -6 l-5 -4 l6 -1 z" fill="{Y}"/>', "time-binding")
# bridge
E["gs_bridge"] = (f'<path d="M0 50 h72 v22 h-72 z" fill="#62a0ea"/><path d="M4 46 Q36 14 68 46" fill="none" stroke="{BR}" stroke-width="5"/><path d="M2 46 h68" stroke="{BR}" stroke-width="5" stroke-linecap="round"/>' + ''.join(f'<line x1="{x}" y1="{46 - (32*(1-((x-36)/32)**2)):.1f}" x2="{x}" y2="46" stroke="{BR}" stroke-width="2.5"/>' for x in (14, 24, 36, 48, 58)), "build bridges")
# no allness: crossed ∀
E["gs_noallness"] = (f'<circle cx="36" cy="36" r="28" fill="{L1}" stroke="{R}" stroke-width="5"/><text x="36" y="50" font-family="{F}" font-size="38" font-weight="800" fill="{INK}" text-anchor="middle">∀</text><path d="M16 56 L56 16" stroke="{R}" stroke-width="5" stroke-linecap="round"/>', "no allness")
# no "is" of identity: crossed =
E["gs_noidentity"] = (f'<circle cx="36" cy="36" r="28" fill="{L1}" stroke="{R}" stroke-width="5"/><path d="M22 30 h28 M22 42 h28" stroke="{INK}" stroke-width="6" stroke-linecap="round"/><path d="M16 56 L56 16" stroke="{R}" stroke-width="5" stroke-linecap="round"/>', "no is of identity")
# evaluation virus
E["gs_virus"] = (''.join(f'<line x1="36" y1="36" x2="{36+28*math.cos(a):.1f}" y2="{36+28*math.sin(a):.1f}" stroke="{G5}" stroke-width="4" stroke-linecap="round"/><circle cx="{36+28*math.cos(a):.1f}" cy="{36+28*math.sin(a):.1f}" r="4.5" fill="{G5}"/>' for a in [k*math.pi/4 for k in range(8)]) + f'<circle cx="36" cy="36" r="19" fill="{G}"/><rect x="33" y="22" width="6" height="16" rx="3" fill="#fff"/><circle cx="36" cy="46" r="3.5" fill="#fff"/>', "evaluation virus")
# SCE brand: circular city
E["sce_city"] = (f'<circle cx="36" cy="36" r="32" fill="#8ff0a4"/><circle cx="36" cy="36" r="25" fill="{L1}" stroke="{B1}" stroke-width="2"/><circle cx="36" cy="36" r="16" fill="#dcebfb" stroke="{B1}" stroke-width="2"/>' + ''.join(f'<line x1="{36+16*math.cos(a):.1f}" y1="{36+16*math.sin(a):.1f}" x2="{36+25*math.cos(a):.1f}" y2="{36+25*math.sin(a):.1f}" stroke="{B1}" stroke-width="2"/>' for a in [k*math.pi/4 for k in range(8)]) + f'<circle cx="36" cy="36" r="8" fill="{B}"/><circle cx="36" cy="36" r="3" fill="#fff"/>', "circular city")
# SCE brand: p2p network
E["sce_nodes"] = (f'<g stroke="{B}" stroke-width="3" stroke-linecap="round"><line x1="36" y1="36" x2="14" y2="16"/><line x1="36" y1="36" x2="58" y2="16"/><line x1="36" y1="36" x2="14" y2="58"/><line x1="36" y1="36" x2="58" y2="58"/><line x1="14" y1="16" x2="58" y2="16" stroke-dasharray="3 5"/></g>' + ''.join(f'<circle cx="{x}" cy="{y}" r="8" fill="{c}"/>' for x, y, c in [(14, 16, G5), (58, 16, P), (14, 58, O), (58, 58, BR)]) + f'<circle cx="36" cy="36" r="11" fill="{B}"/><circle cx="36" cy="36" r="4" fill="#fff"/>', "peer to peer")
# SCE: atom (Atom-Generator)
E["sce_atom"] = (''.join(f'<ellipse cx="36" cy="36" rx="30" ry="11" fill="none" stroke="{P}" stroke-width="3.5" transform="rotate({a} 36 36)"/>' for a in (0, 60, 120)) + f'<circle cx="36" cy="36" r="7" fill="{O}"/>', "atom")
# SCE: scientific method cycle
E["sce_science"] = (''.join(f'<path d="M{36+24*math.cos(a):.1f} {36+24*math.sin(a):.1f} A24 24 0 0 1 {36+24*math.cos(a+1.75):.1f} {36+24*math.sin(a+1.75):.1f}" fill="none" stroke="{c}" stroke-width="6" stroke-linecap="round"/>' for a, c in [(-1.6, B), (0.5, G5), (2.6, O)]) + ''.join(f'<circle cx="{36+24*math.cos(a+1.75):.1f}" cy="{36+24*math.sin(a+1.75):.1f}" r="5" fill="{c}"/>' for a, c in [(-1.6, B), (0.5, G5), (2.6, O)]) + f'<circle cx="36" cy="36" r="9" fill="{INK}"/><path d="M32 36 l3 3 l6 -6" stroke="#fff" stroke-width="2.5" fill="none" stroke-linecap="round"/>', "scientific method")
# SCE: logo — globe with orbit
E["sce_globe"] = (f'<circle cx="36" cy="36" r="26" fill="{B}"/><path d="M18 26 q8 -8 16 -2 q6 5 14 0 q6 -3 8 4 q-6 8 -16 6 q-8 -2 -12 4 q-6 4 -10 -2 z M28 50 q8 -4 16 2 q4 6 -3 9 q-8 2 -13 -3 z" fill="{G}"/><ellipse cx="36" cy="36" rx="34" ry="11" fill="none" stroke="{Y}" stroke-width="3" transform="rotate(-20 36 36)"/>', "SocioCyberneering")

for name, (body, _) in E.items():
    p = os.path.join(OUT, name + ".svg")
    open(p, "w").write(svg(body))
    subprocess.run(["rsvg-convert", "-w", "72", "-h", "72", "-o", os.path.join(OUT, name + ".png"), p], check=True)
# Rust table
with open(os.path.join(root, "src", "emoji_sce.rs"), "w") as f:
    f.write("// Generated by tools/make_emoji.py: SocioCyberneering / general-semantics emoji.\n")
    f.write("pub static SCE: &[(&str, &str, &[u8])] = &[\n")
    for name, (_, desc) in E.items():
        f.write(f'    (":{name}:", "{desc}", include_bytes!("../assets/emoji/sce/{name}.png")),\n')
    f.write("];\n")
print("ok", len(E))
