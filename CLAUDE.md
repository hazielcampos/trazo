# CLAUDE.md

**Read [CONTEXT.md](CONTEXT.md) before doing anything** — it explains the project, the layouts, the
design system, the workflow and the lessons that must not be broken.

Short version of the rules:

- Answer the owner in **Spanish**. Code, comments and commits in English.
- New or experimental work goes on a `test/...` branch; test on Windows in debug
  (`cargo build -p trazo_app --target-dir target/prueba`) and launch it for the owner. No Android builds,
  merges to `main`, pushes or releases unless the owner asks.
- Never close a Trazo window the owner has open; never touch their real projects in tests
  (`tools/captura.py` works on a copy).
- UI changes follow `docs/diseno/` (boards win). Check them with screenshots (`tools/captura.py`) and,
  for real features, an auditor sub-agent that compares captures with the boards.
- Every user-facing string: Spanish in `tr("…")` / `tf!(…)` + English in `crates/trazo_app/src/i18n/en.rs`.
- Run `cargo test --workspace` before committing. End commits with a `Co-Authored-By:` line.
- Never commit signing keys (`*.jks`, `clave.txt`) or anything from the app data folder.
- Keep `CONTEXT.md` and `TODO.md` up to date when something important changes.
