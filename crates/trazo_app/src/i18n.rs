//! Interface languages.
//!
//! Spanish is the source language: every UI string is written in Spanish and
//! wrapped in [`tr`] (or [`tf!`] when it has values to fill in). Other
//! languages look the Spanish text up in their table; a missing entry falls
//! back to Spanish. Tests check that every wrapped string, and every engine
//! name shown in the UI, has a translation.
//!
//! Adding a language: a table module like `en.rs`, a `Lang` variant, and its
//! arms in `code`, `native_name`, `table` and `decimal_comma`.
//!
//! Project data always keeps the Spanish names the app gives by default
//! ("Capa 2"); they are translated only when shown ([`layer_name`]).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;

mod en;

type Table = HashMap<&'static str, &'static str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Es, Lang::En];

    /// Code stored in the settings file (and the system locale prefix).
    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim();
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The language's name written in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
        }
    }

    /// The translation pairs, in table order.
    fn entries(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::Es => &[],
            Lang::En => en::TABLE,
        }
    }

    /// Translations from Spanish; `None` for Spanish itself.
    fn table(self) -> Option<&'static Table> {
        static EN: LazyLock<Table> = LazyLock::new(|| en::TABLE.iter().copied().collect());
        match self {
            Lang::Es => None,
            Lang::En => Some(&EN),
        }
    }

    /// "0,67" or "0.67".
    fn decimal_comma(self) -> bool {
        match self {
            Lang::Es => true,
            Lang::En => false,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(Lang::Es as u8);

pub fn lang() -> Lang {
    Lang::ALL.get(LANG.load(Ordering::Relaxed) as usize).copied().unwrap_or(Lang::Es)
}

pub fn set_lang(l: Lang) {
    LANG.store(l as u8, Ordering::Relaxed);
}

/// Translate a Spanish UI string into the current language.
pub fn tr(s: &str) -> &str {
    match lang().table() {
        None => s,
        Some(t) => t.get(s).copied().unwrap_or(s),
    }
}

/// Translate a message built elsewhere (engine errors): the whole text if it
/// is in the table, otherwise each "part: part" on its own. Entries with `{}`
/// match messages that carry values ("audio a {} Hz no soportado").
pub fn tr_err(msg: &str) -> String {
    let l = lang();
    let Some(t) = l.table() else { return msg.to_string() };
    let one = |s: &str| -> Option<String> { t.get(s).map(|x| x.to_string()).or_else(|| match_template(l.entries(), s)) };
    if let Some(s) = one(msg) {
        return s;
    }
    msg.split(": ").map(|seg| one(seg).unwrap_or_else(|| seg.to_string())).collect::<Vec<_>>().join(": ")
}

/// Find an entry whose `{}` template matches `msg` and fill its translation
/// with the values found there.
fn match_template(entries: &[(&str, &str)], msg: &str) -> Option<String> {
    'entry: for (key, val) in entries {
        if !key.contains("{}") {
            continue;
        }
        let mut parts = key.split("{}");
        let Some(rest) = msg.strip_prefix(parts.next().unwrap_or("")) else { continue };
        let mut rest = rest;
        let mut values = Vec::new();
        let parts: Vec<&str> = parts.collect();
        for (i, p) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let at = if p.is_empty() {
                if last { rest.len() } else { continue 'entry }
            } else if last {
                match rest.strip_suffix(p) {
                    Some(v) => v.len(),
                    None => continue 'entry,
                }
            } else {
                match rest.find(p) {
                    Some(at) => at,
                    None => continue 'entry,
                }
            };
            values.push(&rest[..at]);
            rest = &rest[at + p.len()..];
        }
        if rest.is_empty() && !values.iter().any(|v| v.is_empty()) {
            let args: Vec<&dyn std::fmt::Display> = values.iter().map(|v| v as &dyn std::fmt::Display).collect();
            return Some(fill(val, &args));
        }
    }
    None
}

/// Fill `{}` (in order) or `{0}`, `{1}`… (by position) with `args`.
/// Literal braces aren't supported in templates.
pub fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if after[..end].chars().all(|c| c.is_ascii_digit()) => {
                let idx = if end == 0 {
                    next += 1;
                    next - 1
                } else {
                    after[..end].parse().unwrap_or(usize::MAX)
                };
                if let Some(a) = args.get(idx) {
                    out.push_str(&a.to_string());
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    debug_assert!(next == 0 || next == args.len(), "template {template:?} has {next} slots for {} values", args.len());
    out
}

/// Translate a template and fill in its values: `tf!("Capa {}", n)`.
/// Format numbers beforehand (`num(x, 2)`), the template only takes `{}`.
#[macro_export]
macro_rules! tf {
    ($s:literal $(, $a:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$(&$a as &dyn std::fmt::Display),*])
    };
}

/// A decimal number with the language's separator ("0,67" / "0.67").
pub fn num(v: f32, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if lang().decimal_comma() { s.replace('.', ",") } else { s }
}

/// Layer names the app gives ("Capa 3", "Capa 3 copia") follow the language;
/// names the user typed are shown as they are.
pub fn layer_name(name: &str) -> std::borrow::Cow<'_, str> {
    if lang() == Lang::Es {
        return name.into();
    }
    if let Some(base) = name.strip_suffix(" copia") {
        return tf!("{} copia", layer_name(base)).into();
    }
    for prefix in ["Referencia: ", "Video: "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return format!("{}{rest}", tr(prefix)).into();
        }
    }
    if let Some(n) = name.strip_prefix("Capa ") {
        if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
            return tf!("Capa {}", n).into();
        }
    }
    tr(name).into()
}

/// The default name of the `n`-th layer, as stored in the project.
pub fn default_layer_name(n: usize) -> String {
    format!("Capa {n}")
}

// ---- saved choice -----------------------------------------------------------

const FILE: &str = "idioma.txt";

/// The language saved in `dir`, or `None` the first time the app opens.
pub fn load(dir: &Path) -> Option<Lang> {
    let l = Lang::from_code(&std::fs::read_to_string(dir.join(FILE)).ok()?)?;
    set_lang(l);
    Some(l)
}

pub fn save(dir: &Path, l: Lang) {
    set_lang(l);
    if let Err(e) = std::fs::write(dir.join(FILE), l.code()) {
        log::warn!("could not save the language: {e}");
    }
}

/// Best guess for the first-run screen: the system language if we have it,
/// English for other languages, Spanish when the system doesn't say.
pub fn system_guess() -> Lang {
    let code = system_locale().unwrap_or_default().to_ascii_lowercase();
    if code.is_empty() {
        return Lang::Es;
    }
    Lang::ALL.into_iter().find(|l| code.starts_with(l.code())).unwrap_or(Lang::En)
}

#[cfg(target_os = "android")]
fn system_locale() -> Option<String> {
    crate::android::locale()
}

#[cfg(not(target_os = "android"))]
fn system_locale() -> Option<String> {
    // The OS setting first: a shell's LANG (Git Bash, IDEs) is often a default.
    #[cfg(windows)]
    if let Some(l) = windows_locale() {
        return Some(l);
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() && !v.starts_with("C.") && v != "C" && v != "POSIX" {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    // SAFETY: the buffer is valid for `len` u16 values (LOCALE_NAME_MAX_LENGTH is 85).
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (language, its table) for every language but Spanish.
    fn tables() -> Vec<(Lang, &'static [(&'static str, &'static str)])> {
        vec![(Lang::En, en::TABLE)]
    }

    #[test]
    fn fill_in_order_and_by_position() {
        assert_eq!(fill("Capa {}", &[&3]), "Capa 3");
        assert_eq!(fill("{1} of {0}", &[&"a", &"b"]), "b of a");
        assert_eq!(fill("{x} {}", &[&1]), "{x} 1");
    }

    #[test]
    fn default_layer_names_follow_the_language() {
        set_lang(Lang::En);
        assert_eq!(layer_name("Capa 3"), "Layer 3");
        assert_eq!(layer_name("Capa 3 copia"), "Layer 3 copy");
        assert_eq!(layer_name("Fondo azul"), "Fondo azul");
        assert_eq!(layer_name("Referencia: gato.png"), "Reference: gato.png");
        assert_eq!(layer_name("Rotoscopia"), "Rotoscoping");
        assert_eq!(tr_err("proyecto no válido: imagen de 9000×10: el máximo es 8192 px por lado"), "invalid project: image of 9000×10: the maximum is 8192 px per side");
        assert_eq!(tr_err("audio no compatible: sin pista de audio"), "unsupported audio: no audio track");
        assert_eq!(tr_err("error escribiendo el MP4 (-1): ¿queda espacio?"), "error writing the MP4 (-1): is there free space?");
        set_lang(Lang::Es);
        assert_eq!(layer_name("Capa 3"), "Capa 3");
    }

    #[test]
    fn tables_have_no_duplicate_keys() {
        for (l, table) in tables() {
            let mut seen = std::collections::HashSet::new();
            let dups: Vec<_> = table.iter().filter(|(k, _)| !seen.insert(*k)).map(|(k, _)| *k).collect();
            assert!(dups.is_empty(), "{l:?}: repeated keys {dups:?}");
        }
    }

    /// Every `tr("…")` / `tf!("…")` in the app has an entry in every table,
    /// and templates keep the same number of `{}` slots.
    #[test]
    fn every_string_is_translated() {
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut keys = Vec::new();
        for path in rust_files(Path::new(src)) {
            let text = std::fs::read_to_string(&path).unwrap();
            for key in wrapped_strings(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())) {
                keys.push((path.clone(), key));
            }
        }
        assert!(keys.len() > 300, "the scanner found too few strings");
        for (l, table) in tables() {
            let table: HashMap<&str, &str> = table.iter().copied().collect();
            let mut missing = Vec::new();
            for (path, key) in &keys {
                match table.get(key.as_str()) {
                    None => missing.push(format!("{}: {key:?}", path.display())),
                    Some(t) if t.matches('{').count() != key.matches('{').count() => {
                        missing.push(format!("{}: slots differ in {key:?}", path.display()))
                    }
                    _ => {}
                }
            }
            assert!(missing.is_empty(), "{l:?}: missing strings:\n{}", missing.join("\n"));
        }
    }

    /// Names that come from the engine and are translated when shown.
    #[test]
    fn engine_names_are_translated() {
        use trazo_core::export::{Aspect, ExportFormat};
        let mut names: Vec<String> = Vec::new();
        names.extend(trazo_core::brush::builtin_brushes().into_iter().map(|b| b.name));
        names.extend(trazo_core::brush::BrushCategory::ALL.iter().map(|c| c.label().to_string()));
        names.extend(trazo_core::color::BlendMode::ALL.iter().map(|m| m.label().to_string()));
        names.extend(trazo_core::shapes::ShapeKind::ALL.iter().map(|k| k.label().to_string()));
        names.extend(trazo_core::video::VideoTint::ALL.iter().map(|t| t.label().to_string()));
        names.extend(trazo_core::audio::Phoneme::ALL.iter().map(|p| p.label().to_string()));
        names.extend(Aspect::ALL.iter().map(|a| a.label().to_string()));
        names.extend(
            [ExportFormat::Mp4, ExportFormat::Gif, ExportFormat::PngSequence, ExportFormat::Png, ExportFormat::Avi, ExportFormat::Wav]
                .iter()
                .map(|f| f.label().to_string()),
        );
        // History labels ("Deshacer: …").
        names.extend(
            [
                "trazo", "añadir fotograma", "borrar fotograma", "mover fotograma", "duración de fotograma", "añadir capa",
                "borrar capa", "mover capa", "propiedades de capa", "capa de fondo", "audio", "marcadores", "línea de tiempo",
                "varios cambios",
            ]
            .map(String::from),
        );
        // Preset names kept in constants and translated where shown.
        names.extend(
            [
                "HD 16:9", "Full HD", "Cuadrado", "Vertical / historias", "Pequeño / GIF", "Personalizado", "TikTok / Reels",
                "Instagram", "YouTube", "GIF WhatsApp", "9:16 vertical, 1080p, 30 fps", "4:5, 1080p, 30 fps",
                "1:1, 1080p, 30 fps", "16:9, 1080p, 30 fps", "GIF ligero a 480p", "{} copia", "Referencia: ",
                "Video: ",
            ]
            .map(String::from),
        );
        for (l, table) in tables() {
            let table: HashMap<&str, &str> = table.iter().copied().collect();
            let missing: Vec<_> = names.iter().filter(|n| !table.contains_key(n.as_str())).collect();
            assert!(missing.is_empty(), "{l:?}: engine names without translation: {missing:?}");
        }
    }

    fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(rust_files(&p));
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("i18n.rs") && p.parent().is_none_or(|d| !d.ends_with("i18n")) {
                out.push(p);
            }
        }
        out
    }

    /// String literals passed to `tr(` or `tf!(`. Fails on forms it can't read
    /// (raw strings, unknown escapes) rather than skipping them silently.
    fn wrapped_strings(text: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for pat in ["tr(", "tf!("] {
            let mut rest = text;
            while let Some(i) = rest.find(pat) {
                let before = rest[..i].chars().last();
                rest = &rest[i + pat.len()..];
                // Skip identifiers that only end in "tr", like `attr(`.
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let arg = rest.trim_start();
                if arg.starts_with("r\"") || arg.starts_with("r#") {
                    return Err(format!("raw string in {pat}: use a normal literal"));
                }
                let Some(body) = arg.strip_prefix('"') else { continue }; // a variable, translated elsewhere
                let mut s = String::new();
                let mut chars = body.chars();
                loop {
                    match chars.next() {
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some(c @ ('"' | '\\' | '\'')) => s.push(c),
                            other => return Err(format!("unsupported escape \\{other:?} in {s:?}")),
                        },
                        Some('"') => break,
                        Some(c) => s.push(c),
                        None => return Err("unterminated string".into()),
                    }
                }
                out.push(s);
            }
        }
        Ok(out)
    }
}
