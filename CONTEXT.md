# CONTEXT.md — everything you need to keep working on Trazo

> Read this first at the start of every session (Claude Code loads `CLAUDE.md`, which points here).
> It is the single source of truth about the project: what it is, how it is built, how we work and
> what must never be broken. Keep it up to date when something important changes.

## 1. What Trazo is

Free (GPL-3.0), offline, ad-free app for **hand-drawn frame-by-frame animation**, in the spirit of
FlipaClip / ibis Paint. **100 % Rust** (egui / eframe 0.37), one codebase for:

- **Android** (primary target): phones and tablets, Android 10+ (min SDK 29).
- **Windows** (secondary), and it also builds on macOS / Linux.

Public repo: <https://github.com/hazielcampos/trazo> · branch `main` · releases with the signed APK and
the Windows exe. Current version: see `version` in the root `Cargo.toml` (0.4.0 at the time of writing).

The owner speaks **Spanish**: answer in Spanish. Code, comments and commit messages are in **English**;
user-facing strings are written in **Spanish** and translated (see §6).

## 2. Repository map

```
Cargo.toml              workspace (version lives here)
crates/trazo_core/      engine, no UI — testable on its own
crates/trazo_app/       the app (egui): layouts, input, platform glue
  android/              TrazoPicker.java + picker.dex (Android file picker, see §8)
assets/fonts/           Hanken Grotesk (UI) + JetBrains Mono (frame counter), OFL licences
assets/icon/            app icon (all sizes) + make_icon.py that generates them
docs/capturas/          screenshots used by the README
docs/diseno/            UI design: board sources (fuente/), 1x pictures (tableros/), README with tokens
scripts/                android.sh (emulator), apk-release.sh (signed release APK), build-picker-dex.sh
tools/                  captura.py (screenshots of the app at exact sizes), render_diseno.py (boards → PNG)
TODO.md                 pending work, in Spanish
```

### trazo_core (engine)
| Module | What it does |
|---|---|
| `raster.rs` | sparse tiled images. Tiles: `Tile { id, data: RwLock<Raw \| Packed(LZ4)> }` |
| `stroke.rs`, `brush.rs`, `brush_io.rs`, `preview.rs` | brush engine, built-in brushes, `.animbrush` files, brush previews |
| `document.rs`, `timeline.rs`, `ops.rs`, `history.rs` | project model, per-layer timing (blocks), undoable operations, undo/redo |
| `composite.rs`, `color.rs` | flattening layers + onion skin, blend modes (premultiplied) |
| `fill.rs`, `transform.rs`, `shapes.rs`, `text.rs` | fill, selections / free transform / tweens, shapes, text |
| `audio.rs` | tracks, clips, mixer, decoding (symphonia), peaks for waveforms |
| `video.rs` | video layers for rotoscoping |
| `camera.rs` | start/end framing applied on playback and export |
| `io.rs`, `memory.rs` | `.anim` save/load (zip of PNG + JSON, incremental), memory budget |
| `export.rs` | GIF, PNG, PNG sequence, AVI, WAV + plan/estimates (MP4 lives in the app) |
| `templates.rs` | example projects (ball, blink, walk) |
| `examples/tinta.rs` | demo animation "Viento" (used for screenshots) · `examples/estres.rs` stress project |

### trazo_app (interface)
| File | What it does |
|---|---|
| `app.rs` | app state (`TrazoApp`, `UiState`), top-level layout per size class, shortcuts, debug env vars |
| `widgets.rs` | `SizeClass` (Phone / PhoneLandscape / Wide), sheets, switch, segmented, sliders, buttons |
| `theme.rs` | **design tokens** (colors, radii, fonts) — always use these, never raw colors |
| `icons.rs` + `svgicon.rs` | vector icons; `svgicon` draws the design's SVG paths (24 grid, stroke 1.75) |
| `panels/phone_up.rs` | **the redesigned UI**: phone upright, phone landscape, tablet (studio + clean), PC, tracks, sound in the timeline, drawing menu, color popover, gestures card |
| `panels/phone.rs` | shared phone pieces (project menu, tool options, frame menu, sheets) |
| `panels/layers.rs`, `brushes.rs`, `toolbar.rs` (color panel), `home.rs`, `export.rs` | panels |
| `panels/audio.rs` | sound engine glue: import, recording (countdown, pre-roll), clip operations used by the timeline. Its old standalone panel is no longer shown in the new layouts |
| `panels/timeline.rs` | old desktop timeline + shared helpers (`add_frame`, `selected_block`) |
| `canvas.rs`, `selection.rs`, `tools.rs` | drawing canvas, view (zoom/pan/rotate, auto-fit), selection tools, tool state |
| `audio_engine.rs` | cpal playback (master clock) + microphone |
| `android*.rs`, `windows_mp4.rs` | JNI helpers, MediaCodec MP4 export / video import, Media Foundation MP4 |
| `files.rs`, `import.rs` | picking files (native dialogs) and importing pictures |
| `save.rs`, `versions.rs`, `compactor.rs` | background save, earlier versions, packing cold tiles |
| `i18n.rs`, `i18n/en.rs` | languages (see §6) |

## 3. Layouts (size classes)

`widgets::SizeClass::of(ctx)` decides the layout from the screen size; `platform.touch_first` says if
it is a touch device. In `app.rs` the layout is chosen like this:

| Screen | Layout (all in `panels/phone_up.rs`) | Design boards |
|---|---|---|
| Phone upright (`Phone`) | `top_bar` + floating `island` (tools) + `size_rail` + bottom `dock` (strip of drawings; pull up = tracks) | Tel* |
| Phone on its side (`PhoneLandscape`) | `landscape`: tool rail left, actions rail right, timeline island bottom, panels/tracks as a right card | TelHorizontal, Hor* |
| Tablet (`Wide` + touch), **studio view** | `tablet_panels` + `tablet`: tool rail, inspector (Capas/Pincel/Color tabs), timeline island | Tablet, Tab* |
| Tablet, **clean view** (`ui.tablet_clean`) | same as phone upright (bigger canvas); toggle button in the top bar; saved in `vista_tablet.txt` | — (branch test/rediseno) |
| PC (`Wide`, mouse) | `pc_panels` + `pc_view`: top bar, tool column, docked inspector, timeline with headers, status bar | PC, PCExportar, PCMenus |

The tracks (`tracks_styled` + `TrackStyle::PHONE/TABLET`, PC style inline) are shared by every layout:
one row per layer, then **one row per sound track (waveform)** and a final **"+ Añadir sonido o grabar
tu voz"** row. Selecting a sound clip turns the action bar into the clip bar (`clip_bar`); recording
shows `record_bar`. Setting `audio_ui.show = true` anywhere now just opens the tracks (`app.rs`).

## 4. Design system (short — full guide in `docs/diseno/README.md`)

- Dark and neutral so drawing colors look true. Background `#0E0E0F`, panel `#141416`, sheet
  `#161618`, island/surface `#1B1B1D`, elevated `#252528`, separator `#2F2F33`, control `#3A3A3E`.
- Text `#F2F2F0` / secondary `#A1A1A6` / disabled `#75757B`.
- **Lime accent `#D9F45A` only for what is active** (current tool, current drawing, playhead, toggles on).
  **One white primary button per screen** (`#F2F2F0` with dark text). Danger `#FF7A6E`, recording
  `#FF2D7A`, saved `#5BD6C0`. Track colors in `theme::TRACKS` (dot, cell), sound `theme::AUDIO_TRACK`.
- Radii: control 12, row 14, island 20, sheet 24. Touch targets ≥ 44 pt.
- Fonts: Hanken Grotesk (Regular/Medium/SemiBold/Bold via `theme::font(size, W::…)`), JetBrains Mono
  only for the frame counter (`theme::counter_font`).
- Icons: stroke 1.75 on a 24 grid, round caps; add new ones to `svgicon.rs` (and a no-op arm in `icons.rs`).
- The editable design canvas is a claude.ai artifact (private to the owner); the committed copy is
  `docs/diseno/` (board sources + pictures). When the code and the boards disagree, the boards win
  unless a deviation is documented there.

## 5. How we work (workflow)

1. **New or experimental features go on a `test/...` branch.** Test on Windows in debug first and
   launch the app for the owner to try. Only build Android / merge to `main` when the owner says so.
   Ongoing test branch: `test/rediseno` (tablet clean view is there, not yet in `main`).
2. **Build into a separate target dir** so a running Trazo doesn't lock the exe:
   `cargo build -p trazo_app --target-dir target/prueba` → `target/prueba/debug/trazo.exe`.
   Never kill a Trazo the owner has open; `tools/captura.py` closes only the one it starts.
3. **UI work is checked against the design**: take screenshots with `tools/captura.py` at the board's
   size and compare with `docs/diseno/tableros/<Board>.jpg`. The owner likes an **auditor sub-agent** that
   compares captures with the boards and reports deviations (severity, measurements, fix) — apply them,
   re-capture, repeat until "fiel".
4. For bigger features the owner likes **three parallel review agents**: functional QA, code /
   architecture (long-term scalability), and an animation expert (what's missing for animators).
5. Before committing: `cargo test --workspace` (includes the i18n check) and, ideally,
   `cargo clippy --workspace`. Commits end with a `Co-Authored-By:` line for the AI that helped.
6. Releases (only when asked): bump `version` in `Cargo.toml`, merge to `main`, push, build
   `cargo build --release -p trazo_app` + `scripts/apk-release.sh`, then
   `gh release create vX.Y.Z` with `Trazo-X.Y.Z-android-arm64.apk` and `Trazo-X.Y.Z-windows-x64.exe`
   and bilingual notes (English / Spanish).

### Debug-only environment variables (debug builds, `app.rs`)
| Variable | Effect |
|---|---|
| `TRAZO_TACTIL=1` | pretend the screen is touch-first (shows the tablet layout on a PC) |
| `TRAZO_ESCALA1=1` | 1 pixel per point (screenshots match the boards' sizes) |
| `TRAZO_ABRIR=<file.anim>` | open that project at frame 8 on start |
| `TRAZO_PANEL=pinceles\|capas\|color\|pistas\|menu\|exportar\|gestos\|limpia` | open a panel / state |
| `TRAZO_AUDIO=<file.wav>` | import that sound at start (sound rows, clip bar) |

## 6. Conventions that tests enforce

- **Every user-facing string**: written in Spanish inside `tr("…")` or `tf!("… {}", x)`, with an English
  entry in `crates/trazo_app/src/i18n/en.rs` keyed by the exact Spanish text. `cargo test -p trazo_app`
  fails if one is missing. Engine strings are translated at display with `tr(x)` / `tr_err(msg)`.
  Numbers with decimals: `i18n::num`. Default layer names stay in Spanish in files; `i18n::layer_name`
  translates them.
- egui 0.37 has **no emoji/symbol font**: don't use glyphs like ⌄ ✓ — draw icons (`paint(…)`).
- Use `Id::unique(...)`, `ui.scope_id()`; `egui::Panel` (not the old SidePanel/TopBottomPanel names).
- `egui::Margin` fields are `i8`: margins over 127 overflow — use empty side panels instead (see
  `landscape_panels`).

## 7. Things that must not break (hard-won lessons)

- **Tiles**: never add a blocking `.write()` on a tile lock (only `try_write`): a caller may hold a
  read guard. `px_cold()` reads without unpacking; `px()` unpacks; `px_mut()` unpacks and bumps the tile
  `id`. Tile ids drive `fingerprint()` and the incremental save.
- **Android video decoding**: never through `ImageReader` (unreadable memory on Samsung/Exynos → crash).
  Decoders write YUV into their own output buffers, read with bounds checks.
- **Release signing**: always `scripts/apk-release.sh` (Trazo's own key). The debug key can't update
  installed releases. The key is **not** in the repo (see §9).
- `platforms/android-37` must exist in the SDK (a junction to `android-37.0`; cargo-apk can't parse
  the `.0`). NDK r30 under `%LOCALAPPDATA%\Android\Sdk\ndk\`.
- egui quirk: after rotating, bottom `Panel`s can be laid out off-screen, so landscape overlays are `Area`s.
- The canvas re-fits to its area until the user zooms/moves it (`canvas.rs`, `fitted_for`).
- Test captures must never touch the owner's real projects: `tools/captura.py` copies the demo first.

## 8. Platform notes

- **Android file picker**: NativeActivity can't receive activity results, so `android/TrazoPicker.java`
  (a headless Fragment) opens `ACTION_OPEN_DOCUMENT` and copies the files to the cache. It is compiled
  to `picker.dex` (`scripts/build-picker-dex.sh`, needs javac + d8 from the SDK) and loaded from memory
  with `InMemoryDexClassLoader` (`android.rs`). If it fails, the old in-app browser is the fallback.
  Windows uses the native Explorer dialog (`rfd`).
- **MP4**: Android via MediaCodec (`android_mp4.rs`), Windows via Media Foundation (`windows_mp4.rs`);
  macOS/Linux have no MP4 yet.
- **App data** (projects, brushes, settings, `idioma.txt`, `vista_tablet.txt`):
  Windows `%APPDATA%\trazo\data`, Android `/sdcard/Android/data/com.trazo.app/files`. Not in the repo.
- Test devices of the owner: Samsung Galaxy A37 (arm64). Emulator AVD `Animate_Tablet` for tablets
  (`scripts/android.sh`). Install on a phone with `adb install -r target/release/apk/Trazo.apk`.

## 9. Moving to another PC / deleting this folder

Everything needed to keep working is on GitHub **except two private things**:

1. **The release signing key** — `~/.trazo/trazo-release.jks` and `~/.trazo/clave.txt`.
   Back them up somewhere safe (never in the repo, `.gitignore` blocks `*.jks` and `clave.txt`).
   Losing them means Android users can't update and Google Play can't take new versions.
2. **Your animations and settings** — the app data folder (§8). Copy it if you want them.

Everything else is rebuildable: `target/` (the ~14 GB) is only build output — delete it freely.
The demo project for screenshots is regenerated with
`cargo run --release -p trazo_core --example tinta -- target/capturas/Viento.anim`.

### Setting up a new machine
1. Install Rust (`rustup`), Git and the GitHub CLI (`gh auth login`).
2. `git clone https://github.com/hazielcampos/trazo` and `git fetch --all` (test branches are on GitHub too).
3. Desktop: `cargo run -p trazo_app`.
4. Android: Android Studio (SDK + platform 37 + build-tools + NDK r30), `cargo install cargo-apk`,
   `rustup target add aarch64-linux-android x86_64-linux-android`, the platform junction from §7, and the
   key in `~/.trazo/`. Then `bash scripts/apk-release.sh`.
5. Screenshots: Python 3 + `pip install pillow`; Edge or Chrome for `tools/render_diseno.py`.

## 10. Current state and next steps

- 0.4.0 (main): full redesign on every layout, sound edited inside the timeline, Android system file
  picker, home screen, export sheet, gestures card.
- `test/rediseno`: tablet **clean view** (phone-like UI on tablets, toggle with the studio view) — waiting
  for the owner's OK to merge.
- Pending (details in `TODO.md`): hold-to-pick color and stylus palm rejection (promised by the design,
  not implemented), selecting several drawings, camera track / per-track solo / markers in the PC
  timeline, playback-range handles, scenes for long projects, Google Play release, README screenshots
  of the new UI.
