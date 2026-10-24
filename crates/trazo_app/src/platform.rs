//! Per-platform details (where files go, touch-first layout).

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Platform {
    /// The app's own folder (settings live here).
    pub data_dir: PathBuf,
    /// Folder where projects are stored.
    pub projects_dir: PathBuf,
    /// Folder with the user's own brushes.
    pub brushes_dir: PathBuf,
    /// Folder scanned for files to import (used where there is no file dialog).
    pub import_dir: PathBuf,
    /// Where exported files go on platforms without a save dialog.
    pub export_dir: PathBuf,
    /// True on phones/tablets: bigger controls, touch gestures.
    pub touch_first: bool,
}

impl Platform {
    pub fn desktop() -> Self {
        let base = eframe::storage_dir(crate::APP_ID)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        // The app used to be called "Animate": bring its files along once.
        if let Some(old) = eframe::storage_dir("animate") {
            let new_projects = base.join("proyectos");
            let empty = std::fs::read_dir(&new_projects).map(|mut d| d.next().is_none()).unwrap_or(true);
            if old.is_dir() && empty {
                copy_dir(&old, &base);
            }
        }
        Self::with_dir(base.join("proyectos"), false)
    }

    #[allow(dead_code)] // used on Android only
    pub fn android(data_dir: PathBuf) -> Self {
        Self::with_dir(data_dir.join("proyectos"), true)
    }

    fn with_dir(projects_dir: PathBuf, touch_first: bool) -> Self {
        let base = projects_dir.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let brushes_dir = base.join("pinceles");
        let import_dir = base.join("importar");
        let export_dir = base.join("exportar");
        for d in [&projects_dir, &brushes_dir, &import_dir, &export_dir] {
            if let Err(e) = std::fs::create_dir_all(d) {
                log::warn!("could not create {}: {e}", d.display());
            }
        }
        Self { data_dir: base, projects_dir, brushes_dir, import_dir, export_dir, touch_first }
    }

    /// System file dialog (desktop). Android uses the in-app browser in `files.rs`.
    #[cfg(not(target_os = "android"))]
    pub fn pick_files(&self, title: &str, exts: &[&str]) -> Vec<PathBuf> {
        rfd::FileDialog::new().set_title(title).add_filter(crate::i18n::tr("Archivos"), exts).pick_files().unwrap_or_default()
    }

    /// Called after a picked file was imported. Without a file dialog the file
    /// sits in the import folder, so it is moved aside to avoid importing it twice.
    pub fn imported(&self, path: &std::path::Path) {
        // Only files from the app's own import folder are moved aside.
        #[cfg(target_os = "android")]
        if path.starts_with(&self.import_dir) {
            let done = self.import_dir.join("importados");
            let _ = std::fs::create_dir_all(&done);
            if let Some(name) = path.file_name() {
                let _ = std::fs::rename(path, done.join(name));
            }
        }
        #[cfg(not(target_os = "android"))]
        let _ = path;
    }

    /// Ask where to save a file. On platforms without a dialog the file goes to `import_dir`.
    pub fn save_file(&self, title: &str, default_name: &str, ext: &str) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        {
            rfd::FileDialog::new().set_title(title).set_file_name(default_name).add_filter(ext, &[ext]).save_file()
        }
        #[cfg(target_os = "android")]
        {
            let _ = (title, ext);
            Some(self.export_dir.join(default_name))
        }
    }
}

/// Copy a folder tree (used once to bring files from the old app name).
fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(from) else { return };
    let _ = std::fs::create_dir_all(to);
    for e in entries.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else if !dst.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }
}
