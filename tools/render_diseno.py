"""Render the UI design boards (docs/diseno/fuente/*.dc.html) to PNG with a
headless Chromium browser (Edge or Chrome).

    python tools/render_diseno.py [Board ...]      e.g.  python tools/render_diseno.py TelColor PC

Output: target/diseno/<Board>.png at 2x (target/ is not committed).
The committed 1x pictures live in docs/diseno/tableros/.
"""
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "docs", "diseno", "fuente")
IMG = os.path.join(SRC, "img")
OUT = os.path.join(ROOT, "target", "diseno")
TMP = os.path.join(OUT, "tmp")

# The boards reference pictures by blob id (as stored by the design tool).
BLOBS = {
    "bb5e6c92b21c09b25a134fbfbda5db4c": "viento_grande.jpg",
    "10f76d377b57444960ee88e487676c0a": "viento_00.jpg",
    "fe4c4c4cbae132edfb8a8a7bdeabfb70": "viento_06.jpg",
    "7b740c5a8f2ef8f71a4c1a1696a7ae37": "viento_12.jpg",
    "a1ec0b958ac5bec245de8a034cbbe758": "viento_18.jpg",
}


def browser():
    for p in [
        os.environ.get("BROWSER_EXE", ""),
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        shutil.which("chromium") or "",
        shutil.which("google-chrome") or "",
        shutil.which("microsoft-edge") or "",
    ]:
        if p and os.path.exists(p):
            return p
    sys.exit("No Edge/Chrome found: set BROWSER_EXE")


def main():
    os.makedirs(TMP, exist_ok=True)
    boards = json.load(open(os.path.join(SRC, "canvas.json"), encoding="utf-8"))["boards"]
    names = sys.argv[1:] or [b[: -len(".dc.html")] for b in boards]
    exe = browser()
    for name in names:
        b = boards[name + ".dc.html"]
        src = open(os.path.join(SRC, name + ".dc.html"), encoding="utf-8").read()
        for k, v in BLOBS.items():
            src = src.replace("/_blob/" + k, "file:///" + os.path.join(IMG, v).replace("\\", "/"))
        src = src.replace('<script src="./support.js"></script>', "").replace("<helmet>", "").replace("</helmet>", "")
        tmp = os.path.join(TMP, name + ".html")
        open(tmp, "w", encoding="utf-8").write(src)
        out = os.path.join(OUT, name + ".png")
        subprocess.run(
            [exe, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=2",
             "--allow-file-access-from-files", "--virtual-time-budget=4000",
             f"--window-size={b['w']},{b['h']}", f"--screenshot={out}", "file:///" + tmp.replace("\\", "/")],
            capture_output=True, timeout=120)
        print(name, "ok" if os.path.exists(out) else "FALLÓ")


if __name__ == "__main__":
    main()
