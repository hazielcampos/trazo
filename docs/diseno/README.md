# Trazo — UI design

This folder is the committed copy of Trazo's interface design. The app is built to match these boards;
when code and boards disagree, the boards win unless a deviation is written down below.

- `tableros/` — every board as a 1x picture (JPG). Compare the app with these.
- `fuente/` — the board sources (`*.dc.html`, from the design canvas), `canvas.json` (board sizes) and the
  pictures they use (`img/`). Render them at 2x with `python tools/render_diseno.py [Board…]`
  (output in `target/diseno/`).

## Boards

| Device | Boards |
|---|---|
| Design system | `Sistema` (tokens), `Componentes`, `Iconos` |
| Phone upright (390 × 844) | `Main` (editor), `TelInicio`, `TelGestos`, `TelPinceles`, `TelCapas`, `TelColor`, `TelLinea` (tracks open), `TelFotograma` (drawing menu), `TelExportar` |
| Phone on its side (844 × 390) | `TelHorizontal`, `HorInicio`, `HorPinceles`, `HorCapas`, `HorColor`, `HorLinea`, `HorFotograma`, `HorExportar` |
| Tablet (1280 × 800) | `Tablet`, `TabInicio`, `TabPinceles`, `TabCapaOpciones`, `TabColor`, `TabLinea`, `TabFotograma`, `TabExportar` |
| PC (1440 × 900) | `PC`, `PCMenus`, `PCExportar` |

Where they live in code: `crates/trazo_app/src/panels/phone_up.rs` (all editor layouts), `home.rs`,
`export.rs`, `layers.rs`, `brushes.rs`, `toolbar.rs` (color). Tokens: `theme.rs`. Icons: `svgicon.rs`.

## Tokens (`theme.rs`)

| Token | Value | Use |
|---|---|---|
| `BG` | `#0E0E0F` | app background, canvas surround |
| `PANEL` | `#141416` | PC docked panels |
| `SHEET` | `#161618` | bottom sheets, phone dock |
| `SURFACE` | `#1B1B1D` | floating islands, cards |
| `ELEVATED` | `#252528` | buttons on surfaces, menus |
| `SEPARATOR` | `#2F2F33` | hairlines, slider rails, selected segment |
| `CONTROL` | `#3A3A3E` | grabbers, inactive controls |
| `TEXT` / `TEXT2` / `TEXT3` | `#F2F2F0` / `#A1A1A6` / `#75757B` | text, secondary, disabled |
| `ACCENT` | `#D9F45A` (lime) | **only what is active**: tool, drawing, playhead, toggles on |
| `PRIMARY` | `#F2F2F0` | the one white button per screen (dark text `ON_ACCENT`) |
| `DANGER` / `RECORDING` / `SAVED` | `#FF7A6E` / `#FF2D7A` / `#5BD6C0` | delete, recording, saved dot |
| `TRACKS` | (dot, cell) pairs: blue `#8FC0F0/#35587A`, coral `#EDA287/#7A4636`, green `#AFD39B/#3F5536`, sand `#D4C292/#4F4730`, lilac | timeline rows |
| `AUDIO_TRACK` | `#9DB3EA / #25304D` | sound rows |

Radii: control 12 · row 14 · island 20 · sheet 24. Touch targets ≥ 44 pt.
Type: Hanken Grotesk (titles 20–26 semibold/bold, body 15, secondary 12–13), JetBrains Mono SemiBold only
for the frame counter. Icons: 24 grid, stroke 1.75, round caps and joins.

## Principles

- Dark and neutral so the drawing's colors look true; the canvas is the brightest thing on screen.
- One accent, used only for state. One white primary action per screen.
- Thumb first on phones: tools in a floating island at the bottom, size/opacity on the left edge,
  timeline in a dock you pull up. Tablets get the same controls as islands (studio view) or the phone
  layout (clean view). PC docks everything and adds keyboard shortcuts and a status bar.
- Menus float near what they act on (color over the island, drawing menu over the strip).
- Every gesture has a visible alternative (buttons for undo/redo, menus for long-press actions).

## Deviations from the boards (on purpose)

- **Gestures card**: the board promised "hold still to pick a color" and stylus palm rejection; the app
  doesn't do them yet, so the card lists real gestures (hold a drawing → its menu; two-finger drag).
- **Drawing menu**: no "Seleccionar varios" yet (multi-selection of drawings isn't implemented).
- **PC timeline**: no camera track with keyframes, no per-track solo, no markers, no range handles.
- **Sound** lives inside the timeline on every layout (owner's request), with a clip bar instead of a
  separate audio panel.
- **Tablet clean view** (branch `test/rediseno`): the phone upright layout on tablets; not on a board.
- **Android file picker**: the system picker replaces the in-app browser (no board).
