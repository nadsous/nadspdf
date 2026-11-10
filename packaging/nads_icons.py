"""NadsPDF app icon: the nads.site mark in red with blocky "PDF" lettering below.

Everything is drawn from right-angled polygons (no font), so the SVG masters and the
PNG/ICO/ICNS renders come from the same geometry. Run from the repo root:

    python packaging/nads_icons.py

Needs Pillow. Writes into assets/app-icon/ (file names kept from upstream so every
reference keeps working).
"""

from pathlib import Path

from PIL import Image, ImageDraw

RED = "#EF3E36"
TILE = "#111113"
OUT = Path("assets/app-icon")

# The nads.site mark (apps/web/app/icon.svg), in its own 700-unit space.
MARK = [
    (170.5, 275), (341.3, 275), (341.3, 425), (424.1, 425), (424.1, 339.7), (574.3, 339.7),
    (574.3, 425), (658.7, 425), (658.7, 275), (829.5, 275), (829.5, 360.3), (744.1, 360.3),
    (744.1, 639.7), (829.5, 639.7), (829.5, 725), (658.7, 725), (658.7, 575), (574.3, 575),
    (574.3, 425), (424.1, 425), (424.1, 575), (341.3, 575), (341.3, 725), (170.5, 725),
    (170.5, 639.7), (255.9, 639.7), (255.9, 360.3), (170.5, 360.3),
]

# Blocky letters in a 200 x 250 box: (outer polygon, holes). The stepped corners echo
# the notches of the mark.
LETTERS = {
    "P": (
        [(0, 0), (160, 0), (160, 40), (200, 40), (200, 126), (160, 126), (160, 166), (62, 166), (62, 250), (0, 250)],
        [[(62, 58), (138, 58), (138, 108), (62, 108)]],
    ),
    "D": (
        [(0, 0), (160, 0), (160, 40), (200, 40), (200, 210), (160, 210), (160, 250), (0, 250)],
        [[(62, 58), (138, 58), (138, 192), (62, 192)]],
    ),
    "F": (
        [(0, 0), (200, 0), (200, 58), (62, 58), (62, 100), (172, 100), (172, 156), (62, 156), (62, 250), (0, 250)],
        [],
    ),
}


def layout(size: float, body: float):
    """Polygons (outer, holes) for a `size` canvas whose artwork fills a centred `body` square."""
    k = body / 1024.0
    off = (size - body) / 2.0

    def tr(x, y):
        return (off + x * k, off + y * k)

    shapes = []
    s = 1.1  # mark scale in the 1024 body
    mw, mh = (829.5 - 170.5) * s, (725 - 275) * s
    lh = 230.0
    gap = 64.0
    top = (1024 - (mh + gap + lh)) / 2
    mx = (1024 - mw) / 2
    shapes.append(([tr(mx + (x - 170.5) * s, top + (y - 275) * s) for x, y in MARK], []))

    ls = lh / 250.0
    lw, lgap = 200 * ls, 44.0
    lx = (1024 - (3 * lw + 2 * lgap)) / 2
    ly = top + mh + gap
    for i, ch in enumerate("PDF"):
        outer, holes = LETTERS[ch]
        ox = lx + i * (lw + lgap)
        f = lambda p: tr(ox + p[0] * ls, ly + p[1] * ls)  # noqa: E731
        shapes.append(([f(p) for p in outer], [[f(p) for p in h] for h in holes]))
    return shapes


def tile_radius(body: float) -> float:
    return body * 224 / 1024


def svg(tile: bool) -> str:
    parts = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">']
    if tile:
        parts.append(f'<rect width="1024" height="1024" rx="224" fill="{TILE}"/>')
    d = []
    for outer, holes in layout(1024, 1024 if tile else 1180):
        for poly in [outer, *holes]:
            d.append("M" + "L".join(f"{x:.1f} {y:.1f}" for x, y in poly) + "Z")
    parts.append(f'<path fill="{RED}" fill-rule="evenodd" d="{"".join(d)}"/>')
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def render(size: int, body: float | None = None, tile: bool = True) -> Image.Image:
    """Supersampled raster of the icon; `body` < size leaves a transparent margin (macOS grid)."""
    ss = 4
    big = size * ss
    b = (body if body is not None else size) * ss
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    if tile:
        o = (big - b) / 2
        draw.rounded_rectangle([o, o, o + b - 1, o + b - 1], radius=tile_radius(b), fill=TILE)
    mask = Image.new("L", (big, big), 0)
    md = ImageDraw.Draw(mask)
    for outer, holes in layout(big, b):
        md.polygon(outer, fill=255)
        for h in holes:
            md.polygon(h, fill=0)
    img.paste(Image.new("RGBA", (big, big), RED), (0, 0), mask)
    return img.resize((size, size), Image.LANCZOS)


# The PDF document icon (Explorer, Finder): a sheet with a folded corner and the red mark, so a
# file reads as "a PDF that opens in NadsPDF" rather than as the app itself.
PAGE = [(180, 60), (644, 60), (844, 260), (844, 964), (180, 964)]
FOLD = [(644, 60), (644, 260), (844, 260)]
PAGE_FILL, PAGE_EDGE, FOLD_FILL = "#FAFAFA", "#C9C9D0", "#E2E2E6"


def doc_shapes(size: float):
    """The mark and letters for a `size` document icon: 560/1024 of it, a little below centre."""
    k = size / 1024.0
    return [([(x, y + 70 * k) for x, y in outer], [[(x, y + 70 * k) for x, y in h] for h in holes]) for outer, holes in layout(size, 560 * k)]


def doc_svg() -> str:
    pts = lambda poly: " ".join(f"{x:.1f},{y:.1f}" for x, y in poly)  # noqa: E731
    d = []
    for outer, holes in doc_shapes(1024):
        for poly in [outer, *holes]:
            d.append("M" + "L".join(f"{x:.1f} {y:.1f}" for x, y in poly) + "Z")
    return "\n".join([
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">',
        f'<polygon points="{pts(PAGE)}" fill="{PAGE_FILL}" stroke="{PAGE_EDGE}" stroke-width="24" stroke-linejoin="round"/>',
        f'<polygon points="{pts(FOLD)}" fill="{FOLD_FILL}" stroke="{PAGE_EDGE}" stroke-width="24" stroke-linejoin="round"/>',
        f'<path fill="{RED}" fill-rule="evenodd" d="{"".join(d)}"/>',
        "</svg>",
    ]) + "\n"


def render_doc(size: int) -> Image.Image:
    ss = 4
    big = size * ss
    k = big / 1024.0
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    edge = max(1, round(24 * k))
    draw.polygon([(x * k, y * k) for x, y in PAGE], fill=PAGE_FILL, outline=PAGE_EDGE, width=edge)
    draw.polygon([(x * k, y * k) for x, y in FOLD], fill=FOLD_FILL, outline=PAGE_EDGE, width=edge)
    mask = Image.new("L", (big, big), 0)
    md = ImageDraw.Draw(mask)
    for outer, holes in doc_shapes(big):
        md.polygon(outer, fill=255)
        for h in holes:
            md.polygon(h, fill=0)
    img.paste(Image.new("RGBA", (big, big), RED), (0, 0), mask)
    return img.resize((size, size), Image.LANCZOS)


# The page layout document icon (.nadsmep, the Mise en page tab's files): the same sheet, in a
# yellow-to-orange gradient with a white layout grid, and the nads mark alone in white.
LAYOUT_TOP, LAYOUT_BOTTOM, LAYOUT_EDGE, LAYOUT_FOLD = (255, 214, 64), (255, 112, 24), "#D9620F", "#FFE7A3"
GRID_STEP = 64


def mark_shapes(size: float):
    """The nads mark alone (no "PDF" letters: a page layout is not a PDF), 460/1024 wide, centred
    on the sheet, for a `size` document icon."""
    k = size / 1024.0
    sc = 460.0 / (829.5 - 170.5)
    mw, mh = (829.5 - 170.5) * sc, (725 - 275) * sc
    cx, cy = 512.0, 540.0
    return [([((cx - mw / 2 + (x - 170.5) * sc) * k, (cy - mh / 2 + (y - 275) * sc) * k) for x, y in MARK], [])]


def layout_doc_svg() -> str:
    pts = lambda poly: " ".join(f"{x:.1f},{y:.1f}" for x, y in poly)  # noqa: E731
    d = []
    for outer, holes in mark_shapes(1024):
        for poly in [outer, *holes]:
            d.append("M" + "L".join(f"{x:.1f} {y:.1f}" for x, y in poly) + "Z")
    grid = []
    for x in range(180 + GRID_STEP, 844, GRID_STEP):
        grid.append(f'<line x1="{x}" y1="60" x2="{x}" y2="964"/>')
    for y in range(60 + GRID_STEP, 964, GRID_STEP):
        grid.append(f'<line x1="180" y1="{y}" x2="844" y2="{y}"/>')
    return "\n".join([
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">',
        '<defs><linearGradient id="g" x1="0" y1="0" x2="0.6" y2="1">'
        f'<stop offset="0" stop-color="rgb{LAYOUT_TOP}"/><stop offset="1" stop-color="rgb{LAYOUT_BOTTOM}"/></linearGradient>'
        f'<clipPath id="page"><polygon points="{pts(PAGE)}"/></clipPath></defs>',
        f'<polygon points="{pts(PAGE)}" fill="url(#g)"/>',
        f'<g clip-path="url(#page)" stroke="#FFFFFF" stroke-opacity="0.45" stroke-width="10">{"".join(grid)}</g>',
        f'<polygon points="{pts(PAGE)}" fill="none" stroke="{LAYOUT_EDGE}" stroke-width="24" stroke-linejoin="round"/>',
        f'<polygon points="{pts(FOLD)}" fill="{LAYOUT_FOLD}" stroke="{LAYOUT_EDGE}" stroke-width="24" stroke-linejoin="round"/>',
        f'<path fill="#FFFFFF" fill-rule="evenodd" d="{"".join(d)}"/>',
        "</svg>",
    ]) + "\n"


def render_layout_doc(size: int) -> Image.Image:
    ss = 4
    big = size * ss
    k = big / 1024.0
    page = [(x * k, y * k) for x, y in PAGE]
    # The gradient, top-left yellow to bottom-right orange, cut to the sheet.
    grad = Image.new("RGBA", (big, big))
    gp = grad.load()
    for yy in range(big):
        for xx in range(0, big):
            t = min(1.0, max(0.0, (yy + 0.6 * xx) / (1.6 * big)))
            gp[xx, yy] = tuple(round(a + (b - a) * t) for a, b in zip(LAYOUT_TOP, LAYOUT_BOTTOM)) + (255,)
    sheet = Image.new("L", (big, big), 0)
    ImageDraw.Draw(sheet).polygon(page, fill=255)
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    img.paste(grad, (0, 0), sheet)
    # The grid: every 64 units, every 128 at 24 px and below so it stays a grid, not a blur.
    step = GRID_STEP if size > 24 else GRID_STEP * 2
    lines = Image.new("L", (big, big), 0)
    ld = ImageDraw.Draw(lines)
    w = max(1, round(10 * k))
    for x in range(180 + step, 844, step):
        ld.line([(x * k, 60 * k), (x * k, 964 * k)], fill=115, width=w)
    for y in range(60 + step, 964, step):
        ld.line([(180 * k, y * k), (844 * k, y * k)], fill=115, width=w)
    from PIL import ImageChops
    img.paste(Image.new("RGBA", (big, big), (255, 255, 255, 255)), (0, 0), ImageChops.multiply(lines, sheet))
    draw = ImageDraw.Draw(img)
    edge = max(1, round(24 * k))
    draw.polygon(page, outline=LAYOUT_EDGE, width=edge)
    draw.polygon([(x * k, y * k) for x, y in FOLD], fill=LAYOUT_FOLD, outline=LAYOUT_EDGE, width=edge)
    mask = Image.new("L", (big, big), 0)
    md = ImageDraw.Draw(mask)
    for outer, holes in mark_shapes(big):
        md.polygon(outer, fill=255)
        for h in holes:
            md.polygon(h, fill=0)
    img.paste(Image.new("RGBA", (big, big), (255, 255, 255, 255)), (0, 0), mask)
    return img.resize((size, size), Image.LANCZOS)


def banner(path: Path):
    """The Windows installer's banner (installer-banner.bmp): the icon, the name and what
    it does on the nads base, with the red rule."""
    from PIL import ImageFont

    w, h = 1200, 196
    im = Image.new("RGB", (w, h), TILE)
    d = ImageDraw.Draw(im)
    icon = render(136)
    im.paste(icon, (40, (h - 136) // 2 - 4), icon)
    fonts = Path("C:/Windows/Fonts")
    d.text((206, 40), "NadsPDF", font=ImageFont.truetype(str(fonts / "segoeuib.ttf"), 62), fill="#F4F4F5")
    d.text((210, 118), "Lire, modifier, signer et convertir vos PDF", font=ImageFont.truetype(str(fonts / "segoeui.ttf"), 30), fill="#B4B4BC")
    d.rectangle((0, h - 6, w, h), fill=RED)
    im.save(path)


def main():
    master = svg(tile=True)
    for name in ["pdfcraft.svg", "pdfcraft-small.svg"]:
        (OUT / name).write_text(master, encoding="utf-8")
    (OUT / "hicolor/scalable/apps/ai.storyteller.pdfcraft.svg").write_text(master, encoding="utf-8")
    (OUT / "nadspdf-mark.svg").write_text(svg(tile=False), encoding="utf-8")
    (OUT / "pdfcraft-document.svg").write_text(doc_svg(), encoding="utf-8")

    # macOS: 824 px body on a transparent 1024 canvas.
    render(1024, 824).save(OUT / "pdfcraft-1024.png")
    icns = render(1024, 824)
    icns.save(OUT / "pdfcraft.icns", sizes=[(s, s) for s in (16, 32, 64, 128, 256, 512, 1024)])

    sizes = [16, 24, 32, 48, 64, 128, 256]
    frames = [render(s) for s in sizes]
    frames[-1].save(OUT / "pdfcraft.ico", sizes=[(s, s) for s in sizes], append_images=frames[:-1])
    docs = [render_doc(s) for s in sizes]
    docs[-1].save(OUT / "pdfcraft-document.ico", sizes=[(s, s) for s in sizes], append_images=docs[:-1])
    (OUT / "nadsmep-document.svg").write_text(layout_doc_svg(), encoding="utf-8")
    layouts = [render_layout_doc(s) for s in sizes]
    layouts[-1].save(OUT / "nadsmep-document.ico", sizes=[(s, s) for s in sizes], append_images=layouts[:-1])

    for s in [16, 24, 32, 48, 64, 128, 256, 512]:
        p = OUT / f"hicolor/{s}x{s}/apps/ai.storyteller.pdfcraft.png"
        p.parent.mkdir(parents=True, exist_ok=True)
        render(s).save(p)

    banner(OUT / "installer-banner.bmp")


if __name__ == "__main__":
    main()
