"""Trazo app icon: a tapered white brush stroke with two onion-skin ghosts
on a violet gradient squircle. Renders every size the app needs."""
import math, os, sys
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1]
S = 2048  # work size (supersampled)

def lerp(a, b, t):
    return a + (b - a) * t

def mix(c1, c2, t):
    return tuple(int(round(lerp(a, b, t))) for a, b in zip(c1, c2))

def squircle_mask(size, radius_frac=0.225):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    r = int(size * radius_frac)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=r, fill=255)
    return m

def gradient(size):
    top = (166, 150, 255)    # light violet
    bottom = (82, 58, 214)   # deep indigo
    g = Image.new("RGB", (size, size))
    px = g.load()
    for y in range(size):
        for x in range(size):
            t = (0.35 * x + 0.65 * y) / size
            px[x, y] = mix(top, bottom, min(1.0, max(0.0, t)))
    return g

def bezier(p0, p1, p2, p3, n):
    pts = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        x = u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0]
        y = u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]
        pts.append((x, y, t))
    return pts

def stroke_layer(size, offset, alpha, scale=1.0):
    """Tapered calligraphic stroke (thin start, full body, round tip)."""
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    k = size / 1024
    ox, oy = offset
    # One confident swoosh: rises from lower-left and sweeps up to the right.
    p0 = (230 * k + ox, 735 * k + oy)
    p1 = (430 * k + ox, 820 * k + oy)
    p2 = (650 * k + ox, 650 * k + oy)
    p3 = (790 * k + ox, 255 * k + oy)
    wmax = 104 * k * scale
    for x, y, t in bezier(p0, p1, p2, p3, 1400):
        # pressure: sharp tapered start, full body, gentle thinning at the tip
        ramp = min(1.0, t / 0.42)
        ramp = ramp * ramp * (3 - 2 * ramp)
        tail = 1.0 - 0.28 * max(0.0, (t - 0.75) / 0.25)
        w = wmax * (0.06 + 0.94 * ramp) * tail
        r = w / 2
        d.ellipse([x - r, y - r, x + r, y + r], fill=(255, 255, 255, alpha))
    return layer

def artwork(size, with_bg=True, inset=1.0):
    """Full icon art. `inset` scales the stroke toward the center (adaptive safe zone)."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    if with_bg:
        bg = gradient(size).convert("RGBA")
        # soft highlight top-left
        hl = Image.new("L", (size, size), 0)
        ImageDraw.Draw(hl).ellipse([-size * 0.3, -size * 0.45, size * 0.9, size * 0.55], fill=70)
        hl = hl.filter(ImageFilter.GaussianBlur(size * 0.12))
        white = Image.new("RGBA", (size, size), (255, 255, 255, 255))
        bg = Image.composite(white, bg, hl)
        canvas.paste(bg, (0, 0))
    art = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    k = size / 1024
    for off, a in [((-118 * k, 96 * k), 55), ((-60 * k, 48 * k), 110)]:
        art = Image.alpha_composite(art, stroke_layer(size, off, a))
    # shadow under the main stroke
    main = stroke_layer(size, (0, 0), 255)
    shadow = Image.new("RGBA", (size, size), (30, 10, 90, 0))
    shadow.putalpha(main.getchannel("A").point(lambda v: v * 0.35).filter(ImageFilter.GaussianBlur(18 * k)))
    shadow = shadow.transform(shadow.size, Image.AFFINE, (1, 0, 0, 0, 1, -14 * k))
    art = Image.alpha_composite(art, shadow)
    art = Image.alpha_composite(art, main)
    if inset != 1.0:
        small = art.resize((int(size * inset), int(size * inset)), Image.LANCZOS)
        art = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        o = (size - small.size[0]) // 2
        art.paste(small, (o, o), small)
    canvas = Image.alpha_composite(canvas, art)
    return canvas

os.makedirs(OUT, exist_ok=True)
full = artwork(S)
mask = squircle_mask(S)
icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
icon.paste(full, (0, 0), mask)
icon.resize((1024, 1024), Image.LANCZOS).save(os.path.join(OUT, "icon-1024.png"))
icon.resize((512, 512), Image.LANCZOS).save(os.path.join(OUT, "icon-512.png"))
icon.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "icon-256.png"))
icon.resize((64, 64), Image.LANCZOS).save(os.path.join(OUT, "icon-64.png"))

# Android: legacy squircle mipmaps + adaptive (foreground / background).
res = os.path.join(OUT, "android-res")
dens = {"mdpi": 1, "hdpi": 1.5, "xhdpi": 2, "xxhdpi": 3, "xxxhdpi": 4}
fg = artwork(S, with_bg=False, inset=0.62)
bgimg = artwork(S, with_bg=True, inset=1.0)
bg_only = gradient(S).convert("RGBA")
for name, f in dens.items():
    d = os.path.join(res, f"mipmap-{name}")
    os.makedirs(d, exist_ok=True)
    icon.resize((int(48 * f), int(48 * f)), Image.LANCZOS).save(os.path.join(d, "ic_launcher.png"))
    fg.resize((int(108 * f), int(108 * f)), Image.LANCZOS).save(os.path.join(d, "ic_launcher_foreground.png"))
    bg_only.resize((int(108 * f), int(108 * f)), Image.LANCZOS).save(os.path.join(d, "ic_launcher_background.png"))
d = os.path.join(res, "mipmap-anydpi-v26")
os.makedirs(d, exist_ok=True)
open(os.path.join(d, "ic_launcher.xml"), "w").write(
    '<?xml version="1.0" encoding="utf-8"?>\n'
    '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
    '    <background android:drawable="@mipmap/ic_launcher_background"/>\n'
    '    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
    '    <monochrome android:drawable="@mipmap/ic_launcher_foreground"/>\n'
    '</adaptive-icon>\n')
print("ok")
