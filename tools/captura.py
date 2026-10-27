"""Screenshot the Trazo debug build at an exact window size (Windows only).

Used to compare the app with the design boards (docs/diseno/tableros).

    python tools/captura.py <name> <w> <h> [options]

Options:
    tactil          touch-first layouts (tablet) — TRAZO_TACTIL=1
    noabrir         stay on the home screen
    panel=X         open a panel: pinceles | capas | color | pistas | menu | exportar | gestos | limpia
    audio           load a test voice clip into the project
    espera=N        seconds to wait before the screenshot (default 2.5)
    x,y             click at client coordinates before the screenshot

Needs: a debug build (`cargo build -p trazo_app`), Python 3 with Pillow
(`pip install pillow`). The test project is the demo animation, made once with
`cargo run --release -p trazo_core --example tinta -- target/capturas/Viento.anim`.
Each run works on a fresh copy, so the original is never modified.

Output: target/capturas/<name>.png at 1 pixel per point (TRAZO_ESCALA1).
Only the Trazo it starts is closed; other Trazo windows are left alone.
"""
import ctypes
import math
import os
import random
import shutil
import struct
import subprocess
import sys
import time
import wave
from ctypes import wintypes

from PIL import ImageGrab

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.environ.get("TRAZO_EXE", os.path.join(ROOT, "target", "debug", "trazo.exe"))
OUT = os.path.join(ROOT, "target", "capturas")
DEMO = os.path.join(OUT, "Viento.anim")
WORK = os.path.join(OUT, "trabajo")

ctypes.windll.shcore.SetProcessDpiAwareness(2)
u = ctypes.windll.user32


def test_voice(path):
    """1.6 s of a voice-like tone, for the sound rows."""
    sr = 48000
    w = wave.open(path, "wb")
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(sr)
    frames = bytearray()
    for i in range(int(sr * 1.6)):
        t = i / sr
        env = max(0.0, math.sin(math.pi * ((t * 2.5) % 1))) ** 1.5
        v = env * (0.5 * math.sin(2 * math.pi * 180 * t) + 0.25 * math.sin(2 * math.pi * 360 * t) + 0.1 * (random.random() * 2 - 1))
        frames += struct.pack("<h", int(max(-1.0, min(1.0, v)) * 20000))
    w.writeframes(bytes(frames))
    w.close()


def window_of(pid):
    found = []

    def cb(h, _):
        p = wintypes.DWORD()
        u.GetWindowThreadProcessId(h, ctypes.byref(p))
        buf = ctypes.create_unicode_buffer(64)
        u.GetWindowTextW(h, buf, 64)
        if p.value == pid and u.IsWindowVisible(h) and buf.value == "Trazo":
            found.append(h)
        return True

    u.EnumWindows(ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)(cb), 0)
    return found[0] if found else 0


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    name, w, h = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    extra = sys.argv[4:]
    os.makedirs(WORK, exist_ok=True)
    env = dict(os.environ)
    env["TRAZO_ESCALA1"] = "1"
    if "tactil" in extra:
        env["TRAZO_TACTIL"] = "1"
    if "noabrir" not in extra:
        if not os.path.exists(DEMO):
            sys.exit(f"Missing {DEMO}: cargo run --release -p trazo_core --example tinta -- target/capturas/Viento.anim")
        copy = os.path.join(WORK, "Viento.anim")
        shutil.copyfile(DEMO, copy)
        env["TRAZO_ABRIR"] = copy
    for a in extra:
        if a.startswith("panel="):
            env["TRAZO_PANEL"] = a[6:]
        if a == "audio":
            voice = os.path.join(WORK, "voz.wav")
            test_voice(voice)
            env["TRAZO_AUDIO"] = voice
    clicks = [tuple(map(int, a.split(","))) for a in extra if "," in a]
    wait = next((float(a[7:]) for a in extra if a.startswith("espera=")), 2.5)

    proc = subprocess.Popen([EXE], env=env)
    hwnd = 0
    for _ in range(80):
        time.sleep(0.25)
        hwnd = window_of(proc.pid)
        if hwnd:
            break
    if not hwnd:
        proc.kill()
        sys.exit("no window")
    time.sleep(1.0)
    wr, cr = wintypes.RECT(), wintypes.RECT()
    u.GetWindowRect(hwnd, ctypes.byref(wr))
    u.GetClientRect(hwnd, ctypes.byref(cr))
    bw = (wr.right - wr.left) - cr.right
    bh = (wr.bottom - wr.top) - cr.bottom
    u.ShowWindow(hwnd, 9)
    u.MoveWindow(hwnd, 30, 30, w + bw, h + bh, True)
    time.sleep(wait)

    def origin():
        p = wintypes.POINT(0, 0)
        u.ClientToScreen(hwnd, ctypes.byref(p))
        return p.x, p.y

    for x, y in clicks:
        ox, oy = origin()
        u.SetForegroundWindow(hwnd)
        u.SetCursorPos(ox + x, oy + y)
        time.sleep(0.2)
        u.mouse_event(2, 0, 0, 0, 0)
        time.sleep(0.05)
        u.mouse_event(4, 0, 0, 0, 0)
        time.sleep(1.2)

    u.SetForegroundWindow(hwnd)
    time.sleep(0.6)
    ox, oy = origin()
    u.GetClientRect(hwnd, ctypes.byref(cr))
    im = ImageGrab.grab((ox, oy, ox + cr.right, oy + cr.bottom), all_screens=True)
    out = os.path.join(OUT, name + ".png")
    im.save(out)
    print(out, im.size)
    proc.kill()


if __name__ == "__main__":
    main()
