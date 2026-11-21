#!/usr/bin/env python3
"""README art: hero banner and the "100 people in one voice channel" scheme."""
import math, os, random, subprocess
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(root, "docs", "img"); os.makedirs(OUT, exist_ok=True)
F = "Inter, Cantarell, 'Noto Sans', sans-serif"
BLUE, BLUE1, BLUE5, GREEN, GREEN5, ORANGE, PURP, RED, YEL, BROWN = "#3584e4", "#99c1f1", "#1a5fb4", "#2ec27e", "#26a269", "#ff7800", "#9141ac", "#e01b24", "#f6d32d", "#865e3c"
INK, INK2 = "#241f31", "#5e5c64"
PEOPLE = ["#3584e4", "#9141ac", "#26a269", "#c64600", "#865e3c", "#1c71d8", "#613583", "#e5a50a"]

DEFS = '''<defs><filter id="sh" x="-30%" y="-30%" width="160%" height="160%"><feDropShadow dx="0" dy="3" stdDeviation="4" flood-color="#000" flood-opacity=".14"/></filter>
<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#eaf2fd"/><stop offset="1" stop-color="#f6f5f4"/></linearGradient>
<linearGradient id="hero" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#1a5fb4"/><stop offset=".6" stop-color="#3584e4"/><stop offset="1" stop-color="#62a0ea"/></linearGradient>
<radialGradient id="glow"><stop offset="0" stop-color="#fff" stop-opacity=".35"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient></defs>'''

def person(x, y, r, c, ring=None):
    s = f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r}" fill="{c}"/>'
    s += f'<circle cx="{x:.1f}" cy="{y - r*0.28:.1f}" r="{r*0.32:.1f}" fill="#fff"/><path d="M{x - r*0.55:.1f} {y + r*0.55:.1f} a{r*0.55:.1f} {r*0.42:.1f} 0 0 1 {r*1.1:.1f} 0" fill="#fff"/>'
    if ring:
        s = f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r+4}" fill="none" stroke="{ring}" stroke-width="3"/>' + s
    return s

def waves(x, y, c):
    return ''.join(f'<path d="M{x+14+k*6} {y-8-k*4} q{6+k*3} {8+k*4} 0 {16+k*8}" fill="none" stroke="{c}" stroke-width="2.5" stroke-linecap="round" opacity="{1-k*0.28}"/>' for k in range(3))

def voice100(lang):
    W, H = 960, 700
    cx, cy, R = 480, 352, 200
    T = {
        "ru": ["100 человек в одном голосовом канале", "Говорит (Opus ≈ 32 кбит/с)", "Relay — участник с хорошим каналом", "Слушатель", "Поток между relay", "8 relay + 92 слушателя = 100 · сервера нет"],
        "en": ["100 people in one voice channel", "Speaking (Opus ≈ 32 kbit/s)", "Relay: a participant with a good link", "Listener", "Stream between relays", "8 relays + 92 listeners = 100 · no server"],
    }[lang]
    b = f'<rect width="{W}" height="{H}" rx="24" fill="url(#bg)"/>'
    b += f'<text x="40" y="52" font-family="{F}" font-size="24" font-weight="700" fill="{INK}">{T[0]}</text>'
    b += f'<circle cx="{cx}" cy="{cy}" r="{R+80}" fill="{BLUE1}" opacity=".12"/>'
    relays = [(cx + R*math.cos(2*math.pi*k/8 - math.pi/2), cy + R*math.sin(2*math.pi*k/8 - math.pi/2)*0.86) for k in range(8)]
    # relay mesh: ring + chords
    for i in range(8):
        for j in (i+1, i+3):
            a, c = relays[i], relays[j % 8]
            b += f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{c[0]:.1f}" y2="{c[1]:.1f}" stroke="{BLUE}" stroke-width="{3 if j==i+1 else 1.6}" stroke-dasharray="{"0" if j==i+1 else "4 6"}" opacity="{.75 if j==i+1 else .45}" stroke-linecap="round"/>'
    counts = [12, 12, 11, 12, 11, 12, 11, 11]
    speakers = {(0, 3), (3, 7), (6, 2)}
    rnd = random.Random(7)
    lis = ''
    links = ''
    for k, (rx, ry) in enumerate(relays):
        out_a = math.atan2(ry - cy, rx - cx)
        n = counts[k]
        for i in range(n):
            a = out_a + (i - (n - 1) / 2) * (2.6 / n) * 1.25
            d = 62 + (i % 2) * 18
            x, y = rx + d*math.cos(a), ry + d*math.sin(a)
            sp = (k, i) in speakers
            col = ORANGE if sp else PEOPLE[rnd.randrange(len(PEOPLE))]
            links += f'<line x1="{rx:.1f}" y1="{ry:.1f}" x2="{x:.1f}" y2="{y:.1f}" stroke="{ORANGE if sp else "#c0bfbc"}" stroke-width="{3.5 if sp else 1.4}"/>'
            lis += person(x, y, 9, col, ring=ORANGE if sp else None)
            if sp:
                lis += waves(x, y, ORANGE)
    b += links + lis
    for (rx, ry) in relays:
        b += f'<g filter="url(#sh)">{person(rx, ry, 17, GREEN5, ring=GREEN)}</g>'
    # centre: one channel
    b += f'<g filter="url(#sh)"><rect x="{cx-70}" y="{cy-22}" width="140" height="44" rx="22" fill="#fff"/></g>'
    b += f'<path d="M{cx-48} {cy-6} v12 h6 l8 7 v-26 l-8 7 z" fill="{BLUE}"/><path d="M{cx-28} {cy-7} q5 7 0 14" stroke="{BLUE}" stroke-width="2.5" fill="none" stroke-linecap="round"/>'
    b += f'<text x="{cx-16}" y="{cy+6}" font-family="{F}" font-size="16" font-weight="700" fill="{INK}">lounge</text>'
    # legend
    ly = H - 70
    items = [(ORANGE, T[1], "sp"), (GREEN5, T[2], "relay"), ("#9141ac", T[3], "lis"), (BLUE, T[4], "line")]
    x = 40
    for c, t, kind in items:
        if kind == "line":
            b += f'<line x1="{x}" y1="{ly}" x2="{x+26}" y2="{ly}" stroke="{c}" stroke-width="3" stroke-linecap="round"/>'
        else:
            b += person(x + 12, ly, 11 if kind != "relay" else 13, c, ring=ORANGE if kind == "sp" else (GREEN if kind == "relay" else None))
        b += f'<text x="{x+36}" y="{ly+5}" font-family="{F}" font-size="14" fill="{INK2}">{t}</text>'
        x += 36 + len(t) * 7.4 + 26
    b += f'<text x="40" y="{H-28}" font-family="{F}" font-size="14" font-weight="700" fill="{INK2}">{T[5]}</text>'
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">{DEFS}{b}</svg>'

def hero(lang):
    W, H = 960, 320
    T = {"ru": ["Сеть, которую держат люди", "Без серверов. Без банов. Без лимитов.", "Вы отвечаете за будущее."],
         "en": ["A network held by people", "No servers. No bans. No limits.", "You are responsible for the future."]}[lang]
    b = f'<rect width="{W}" height="{H}" rx="24" fill="url(#hero)"/><circle cx="740" cy="160" r="220" fill="url(#glow)"/>'
    b += f'<text x="48" y="92" font-family="{F}" font-size="18" font-weight="700" fill="#fff" opacity=".8" letter-spacing="2">SOCIOCYBERNEERING INC. · CYBERCORD</text>'
    b += f'<text x="48" y="148" font-family="{F}" font-size="38" font-weight="800" fill="#fff">{T[0]}</text>'
    b += f'<text x="48" y="194" font-family="{F}" font-size="22" fill="#fff" opacity=".95">{T[1]}</text>'
    b += f'<text x="48" y="232" font-family="{F}" font-size="22" font-weight="700" fill="{YEL}">{T[2]}</text>'
    # people network on the right
    cx, cy = 760, 160
    rnd = random.Random(3)
    pts = []
    for ring, n in ((62, 7), (118, 13)):
        for i in range(n):
            a = 2*math.pi*i/n + ring
            pts.append((cx + ring*math.cos(a), cy + ring*math.sin(a)*0.92))
    for i, p in enumerate(pts):
        for q in pts[i+1:]:
            if math.dist(p, q) < 70:
                b += f'<line x1="{p[0]:.1f}" y1="{p[1]:.1f}" x2="{q[0]:.1f}" y2="{q[1]:.1f}" stroke="#fff" stroke-width="1.6" opacity=".55"/>'
    for p in pts[:7]:
        b += f'<line x1="{cx}" y1="{cy}" x2="{p[0]:.1f}" y2="{p[1]:.1f}" stroke="#fff" stroke-width="2" opacity=".7"/>'
    for i, p in enumerate(pts):
        b += f'<circle cx="{p[0]:.1f}" cy="{p[1]:.1f}" r="15" fill="#fff"/>' + person(p[0], p[1], 12, PEOPLE[i % len(PEOPLE)])
    b += f'<circle cx="{cx}" cy="{cy}" r="26" fill="#fff"/><circle cx="{cx}" cy="{cy}" r="18" fill="{GREEN}"/><path d="M{cx-8} {cy} l6 6 l10 -12" stroke="#fff" stroke-width="3.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>'
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">{DEFS}{b}</svg>'

for lang in ("ru", "en"):
    sfx = "" if lang == "ru" else ".en"
    for name, fn in (("voice-100", voice100), ("hero", hero)):
        p = os.path.join(OUT, f"{name}{sfx}.svg")
        open(p, "w").write(fn(lang))
        subprocess.run(["rsvg-convert", "-o", f"/tmp/art/{name}{sfx}.png", p], check=True)
print("ok")
