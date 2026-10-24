# Contributing to Trazo

Thanks for helping! Trazo is a free (GPL-3.0) frame-by-frame animation app written 100 % in Rust.
Before changing anything, read **[CONTEXT.md](CONTEXT.md)**: it has the architecture, the layouts, the
design system and the rules that keep the app stable.

*¿Hablas español? Todo lo de abajo aplica igual; puedes abrir issues y PR en español.*

## Getting started

```bash
git clone https://github.com/hazielcampos/trazo
cd trazo
cargo run -p trazo_app            # desktop
cargo test --workspace            # tests (includes the translation check)
```

Android needs the SDK, NDK r30 and `cargo-apk` — see CONTEXT.md §9.

## Working with Claude Code

The repo is set up so that an AI assistant can pick up the work with full context:

1. Open the folder with Claude Code. It loads `CLAUDE.md`, which tells it to read `CONTEXT.md`.
2. Ask for the change. It will work on a `test/...` branch, test on desktop and show you the result.
3. For UI work, it compares screenshots of the app (`tools/captura.py`) with the design boards in
   `docs/diseno/tableros/`, ideally with an auditor sub-agent, and fixes deviations.
4. Review, then open a pull request against `main`.

## Rules for every change

- **Branches**: `test/<topic>` for experiments, then a pull request to `main`.
- **Strings**: every text the user sees is written in Spanish inside `tr("…")` or `tf!("… {}", x)` and
  gets an English entry in `crates/trazo_app/src/i18n/en.rs`. `cargo test` fails otherwise.
- **Design**: use the tokens in `crates/trazo_app/src/theme.rs` (colors, radii, fonts) and the icons in
  `svgicon.rs`. Follow `docs/diseno/README.md`. Lime accent only for what is active; one white button per
  screen; touch targets of at least 44 pt.
- **Engine vs app**: drawing, timeline, audio, file formats and exporters go in `crates/trazo_core` (no
  UI, with tests). Layout and input go in `crates/trazo_app`.
- **Before sending**: `cargo test --workspace` and `cargo clippy --workspace`. Describe what you tested
  (desktop, phone, tablet) and attach screenshots for UI changes.
- **Never commit** signing keys, `.apk` files, build output or personal projects.

## Adding a language

Copy `crates/trazo_app/src/i18n/en.rs`, translate the right-hand column and add the language to `Lang`
in `crates/trazo_app/src/i18n.rs`. `cargo test` tells you if a string is missing.

## Reporting bugs

Open an issue with the device (phone/tablet/PC, model, Android or Windows version), Trazo version,
steps to reproduce, and a screenshot or the `.anim` file if you can share it.
