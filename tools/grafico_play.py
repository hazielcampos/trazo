"""Google Play feature graphic (1024 x 500) from the demo animation.

    python tools/grafico_play.py   ->  docs/play/grafico-destacado.png
"""
import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG = os.path.join(ROOT, "docs", "diseno", "fuente", "img")
FONTS = os.path.join(ROOT, "assets", "fonts")
OUT = os.path.join(ROOT, "docs", "play")

W, H = 1024, 500
BG = (14, 14, 15)
SURFACE = (27, 27, 29)
TEXT = (242, 242, 240)
TEXT2 = (161, 161, 166)
ACCENT = (217, 244, 90)


def font(name, size):
    return ImageFont.truetype(os.path.join(FONTS, f"HankenGrotesk-{name}.ttf"), size)


def rounded(im, r):
    mask = Image.new("L", im.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, im.width - 1, im.height - 1), r, fill=255)
    out = Image.new("RGBA", im.size, (0, 0, 0, 0))
    out.paste(im.convert("RGBA"), (0, 0), mask)
    return out


def card(path, h, r, dim=0.0):
    im = Image.open(path).convert("RGB")
    im = im.resize((round(im.width * h / im.height), h), Image.LANCZOS)
    if dim:
        im = Image.blend(im, Image.new("RGB", im.size, BG), dim)
    return rounded(im, r)


def shadow(canvas, box, r, blur, alpha):
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).rounded_rectangle(box, r, fill=(0, 0, 0, alpha))
    canvas.alpha_composite(layer.filter(ImageFilter.GaussianBlur(blur)))


def main():
    os.makedirs(OUT, exist_ok=True)
    g = Image.new("RGBA", (W, H), BG + (255,))
    d = ImageDraw.Draw(g)

    # Soft light behind the drawings.
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse((560, 40, 1000, 470), fill=(60, 60, 58, 120))
    g.alpha_composite(glow.filter(ImageFilter.GaussianBlur(70)))

    # Frames of the animation, like onion skin: older ones fade behind.
    back = [("viento_06.jpg", 236, 0.62, (578, 132)), ("viento_12.jpg", 262, 0.42, (634, 112))]
    for name, h, dim, (x, y) in back:
        c = card(os.path.join(IMG, name), h, 14, dim)
        shadow(g, (x, y + 8, x + c.width, y + c.height + 8), 14, 14, 140)
        g.alpha_composite(c, (x, y))
    front = card(os.path.join(IMG, "viento_grande.jpg"), 330, 18)
    fx, fy = 708, 70
    shadow(g, (fx, fy + 14, fx + front.width, fy + front.height + 14), 18, 22, 170)
    g.alpha_composite(front, (fx, fy))
    d = ImageDraw.Draw(g)
    d.rounded_rectangle((fx - 4, fy - 4, fx + front.width + 3, fy + front.height + 3), 22, outline=ACCENT, width=4)

    # A small timeline under the drawings: the current one lit.
    tx, ty = 566, 432
    cells = 9
    for i in range(cells):
        x = tx + i * 44
        on = i == 6
        d.rounded_rectangle((x, ty, x + 38, ty + 26), 7, fill=(143, 192, 240) if on else (53, 88, 122))
        if on:
            d.rounded_rectangle((x - 3, ty - 3, x + 41, ty + 29), 9, outline=ACCENT, width=3)
    px = tx + 6 * 44 + 19
    d.line((px, ty - 14, px, ty + 40), fill=ACCENT, width=3)

    # Logo, name and one line.
    icon = Image.open(os.path.join(ROOT, "assets", "icon", "icon-512.png")).convert("RGBA").resize((92, 92), Image.LANCZOS)
    g.alpha_composite(icon, (72, 118))
    d.text((72, 228), "Trazo", font=font("Bold", 92), fill=TEXT)
    d.text((76, 344), "Anima a mano,", font=font("Medium", 36), fill=TEXT2)
    d.text((76, 388), "cuadro a cuadro.", font=font("Medium", 36), fill=TEXT2)

    out = os.path.join(OUT, "grafico-destacado.png")
    g.convert("RGB").save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
