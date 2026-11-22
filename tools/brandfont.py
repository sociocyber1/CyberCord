"""Brand lettering: "SocioCyberneering Inc." is always set in Montserrat,
letter-spaced (~0.12 em). This turns text into SVG paths, so pictures look
the same everywhere, even where Montserrat is not installed."""
import os
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_fonts = {}

def _font(weight):
    if weight not in _fonts:
        _fonts[weight] = TTFont(os.path.join(ROOT, "assets", "fonts", f"Montserrat-{weight}.ttf"))
    return _fonts[weight]

def text_path(text, x, y, size, fill="#241f31", tracking=0.12, weight="Regular", anchor="start", opacity=1.0):
    """SVG <path> for `text` with its baseline at (x, y)."""
    f = _font(weight)
    upm = f["head"].unitsPerEm
    cmap = f.getBestCmap()
    gs = f.getGlyphSet()
    hmtx = f["hmtx"]
    k = size / upm
    names = [cmap.get(ord(c), ".notdef") for c in text]
    width = sum(hmtx[n][0] for n in names) * k + tracking * size * (len(text) - 1)
    if anchor == "middle":
        x -= width / 2
    elif anchor == "end":
        x -= width
    pen = SVGPathPen(gs)
    cx = x
    for n in names:
        tp = TransformPen(pen, (k, 0, 0, -k, cx, y))
        gs[n].draw(tp)
        cx += hmtx[n][0] * k + tracking * size
    return f'<path d="{pen.getCommands()}" fill="{fill}" opacity="{opacity}"/>', width

if __name__ == "__main__":
    import subprocess
    out = os.path.join(ROOT, "assets", "brand")
    os.makedirs(out, exist_ok=True)
    for name, col in (("wordmark-dark", "#241f31"), ("wordmark-light", "#ffffff")):
        p, w = text_path("SocioCyberneering Inc.", 8, 40, 32, fill=col)
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="{w+16:.0f}" height="52" viewBox="0 0 {w+16:.0f} 52">{p}</svg>'
        open(os.path.join(out, name + ".svg"), "w").write(svg)
        subprocess.run(["rsvg-convert", "-h", "104", "-o", os.path.join(out, name + ".png"), os.path.join(out, name + ".svg")], check=True)
    print("ok")
